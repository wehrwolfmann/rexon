//! Перевод интерфейса Rexon: один каталог gettext на весь продукт.
//!
//! ЗАЧЕМ. Раньше подписи были зашиты по-русски прямо в код, и добавить
//! польский или чешский значило бы править сотни мест. Теперь исходный
//! язык строк — английский (так принято в gettext и так их поймёт любой
//! переводчик), а каждый язык — один файл `po/<код>.po`. Добавить язык =
//! положить `.po` и вписать код в `po/LINGUAS`; код не трогается.
//!
//! КАК УСТРОЕНО.
//! * Строки в Rust помечаются функциями этого модуля: [`tr`], [`trc`]
//!   (с контекстом), [`ntr`] (форма по числу), [`trf`] (с подстановкой
//!   `%1`, `%2`…) и маркером [`n_`] для таблиц и констант, где функцию
//!   позвать нельзя. Их имена — ключевые слова `xgettext` в
//!   `scripts/i18n-update.sh`: переименовать функцию = переучить скрипт.
//! * Заполнители — в стиле Qt (`%1`, `%n`), а не `{}`: одна и та же строка
//!   каталога уходит и сюда, и в QML (`qsTranslate`), и переводчик видит
//!   один синтаксис.
//! * Каталог читается из `.mo` чистым Rust — без libintl и без
//!   `setlocale`: язык Rexon выбирается своей настройкой и не обязан
//!   совпадать с локалью процесса.
//! * QML читает ТОТ ЖЕ `.mo`: `qsTranslate("rexon", …)` попадает в
//!   переводчик окна (`QTranslator`-наследник в
//!   `rexon-gui/cpp/rexon_gui_factory.cpp`), а тот спрашивает
//!   [`qt_translate`]. Второго каталога (`.qm`) у Rexon нет: перевод
//!   хранится в одном месте, и разойтись QML с Rust не может.
//!
//! ВЫБОР ЯЗЫКА: настройка пользователя (`ui_language` в `settings.json`) ▸
//! системная локаль (`LANGUAGE`, затем первое непустое из `LC_ALL`,
//! `LC_MESSAGES`, `LANG`), с откатом до базового языка (`pt_BR` → `pt`) ▸
//! английский. Английский — исходный язык, каталога у него нет.
//!
//! ГДЕ ИЩУТСЯ КАТАЛОГИ (первый найденный): `$REXON_LOCALEDIR` ▸ `locale/`
//! рядом с исполняемым файлом (туда кладёт сборка `cargo build-app`, это
//! путь дерева разработки) ▸ `/usr/share/locale` (пакет). Внутри —
//! обычная раскладка gettext: `<язык>/LC_MESSAGES/rexon.mo`.
//!
//! Формат `.mo` — по руководству GNU gettext, раздел «The Format of GNU MO
//! Files»; выражение `Plural-Forms` — там же, раздел «Plural forms».

use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

/// Имя каталога (домен gettext): `rexon.mo`, `rexon.pot`.
pub const DOMAIN: &str = "rexon";

/// Контекст Qt, на который отвечает переводчик окна (см. [`qt_translate`]).
/// QML обязан звать `qsTranslate("rexon", "…")`, а не `qsTr("…")`: у `qsTr`
/// контекст — имя файла, а у вендорного waywallen в том же процессе есть
/// свои контексты с теми же именами (`SettingsPage`), и каталоги
/// пересекались бы. Строки чужих контекстов переводчик не трогает.
pub const QT_CONTEXT: &str = "rexon";

/// Исходный язык строк. Для него каталога нет и быть не должно.
pub const SOURCE_LANGUAGE: &str = "en";

/// Переменная окружения с каталогом переводов для разработки и проверки.
pub const ENV_LOCALEDIR: &str = "REXON_LOCALEDIR";

/// Системный каталог переводов (туда ставит пакет).
pub const SYSTEM_LOCALEDIR: &str = "/usr/share/locale";

/// Ключ настройки языка в `settings.json`. Пусто или `"system"` — как в
/// системе.
pub const SETTINGS_KEY: &str = "ui_language";

// ============================================================================
// Пометки строк
// ============================================================================

/// Маркер «эта строка переводится», ничего не делающий сам.
///
/// Для таблиц и `const`, где звать [`tr`] нельзя: строку извлечёт
/// `xgettext`, а переведёт её место показа — `tr(STEP.title)`.
pub const fn n_(msgid: &'static str) -> &'static str {
    msgid
}

/// Перевести строку.
pub fn tr(msgid: &str) -> String {
    lookup(None, msgid).unwrap_or_else(|| msgid.to_string())
}

/// Перевести строку с контекстом — когда одно английское слово значит
/// разное («Open» — глагол на кнопке и прилагательное в статусе).
pub fn trc(context: &str, msgid: &str) -> String {
    lookup(Some(context), msgid).unwrap_or_else(|| msgid.to_string())
}

/// Перевести строку и подставить `%1`, `%2`… по порядку.
pub fn trf(msgid: &str, args: &[&dyn Display]) -> String {
    fill(&tr(msgid), args)
}

/// Форма строки по числу `n` с подстановкой `%n`.
///
/// Форм у языка бывает от одной до шести, и какая из них нужна, решает
/// выражение `Plural-Forms` каталога — не код. Без каталога работает
/// английское правило: одна вещь — `singular`, остальное — `plural`.
pub fn ntr(singular: &str, plural: &str, n: u64) -> String {
    let form = catalog()
        .and_then(|c| c.plural_lookup(singular, n))
        .unwrap_or_else(|| if n == 1 { singular } else { plural }.to_string());
    form.replace("%n", &n.to_string())
}

/// Подставить `%1`…`%99` в уже переведённый шаблон.
///
/// Номер берётся целиком (`%12` — двенадцатый, а не первый с «2»), чужой
/// номер и одинокий `%` остаются как есть: лучше видимый заполнитель, чем
/// молча пропавшее слово.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let digits = bytes[i + 1..]
                .iter()
                .take(2)
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits > 0 {
                let index: usize = template[i + 1..i + 1 + digits].parse().unwrap_or(0);
                if index >= 1 && index <= args.len() {
                    out.push_str(&args[index - 1].to_string());
                    i += 1 + digits;
                    continue;
                }
            }
        }
        // Байт за байтом нельзя — UTF-8; идём до следующего `%`. Сам `%`
        // однобайтовый, поэтому `i + 1` после него — граница символа.
        let from = if bytes[i] == b'%' { i + 1 } else { i };
        let next = template[from..].find('%').map_or(bytes.len(), |p| from + p);
        out.push_str(&template[i..next]);
        i = next;
    }
    out
}

// ============================================================================
// Каталог .mo
// ============================================================================

/// Разобранный каталог одного языка.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// Ключ — `msgid` (у форм по числу — единственное число), с
    /// контекстом через `\u{4}`, как пишет его `msgfmt`.
    messages: HashMap<String, Vec<String>>,
    plural: Option<PluralExpr>,
    nplurals: usize,
}

/// Магическое число `.mo` в порядке байтов файла.
const MO_MAGIC: u32 = 0x9504_12de;

impl Catalog {
    /// Прочитать `.mo` с диска.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse_mo(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Разобрать `.mo` из памяти. Любая порча — ошибка, а не паника:
    /// каталог лежит на диске и мог обрезаться.
    pub fn parse_mo(data: &[u8]) -> Result<Self, String> {
        let word = |at: usize, big: bool| -> Result<u32, String> {
            let raw: [u8; 4] = data
                .get(at..at + 4)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| format!("файл обрывается на смещении {at}"))?;
            Ok(if big { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) })
        };
        let big = match word(0, false)? {
            MO_MAGIC => false,
            m if m.swap_bytes() == MO_MAGIC => true,
            _ => return Err("это не каталог .mo (нет магического числа)".into()),
        };
        let count = word(8, big)? as usize;
        let originals = word(12, big)? as usize;
        let translations = word(16, big)? as usize;
        let string_at = |table: usize, index: usize| -> Result<&str, String> {
            let len = word(table + index * 8, big)? as usize;
            let off = word(table + index * 8 + 4, big)? as usize;
            let raw = data
                .get(off..off.checked_add(len).ok_or("переполнение смещения")?)
                .ok_or_else(|| format!("строка {index} выходит за конец файла"))?;
            std::str::from_utf8(raw).map_err(|_| format!("строка {index} не в UTF-8"))
        };

        let mut messages = HashMap::with_capacity(count);
        let mut header = "";
        for index in 0..count {
            let original = string_at(originals, index)?;
            let translated = string_at(translations, index)?;
            if original.is_empty() {
                header = translated;
                continue;
            }
            // У форм по числу оригинал — «единственное\0множественное»,
            // искать же будут по единственному.
            let key = original.split('\0').next().unwrap_or(original);
            let forms = translated.split('\0').map(str::to_string).collect();
            messages.insert(key.to_string(), forms);
        }

        let (nplurals, plural) = parse_plural_header(header);
        Ok(Self { messages, plural, nplurals })
    }

    /// Перевод строки; `None` — в каталоге её нет (или перевод пуст).
    pub fn get(&self, context: Option<&str>, msgid: &str) -> Option<&str> {
        let forms = match context {
            Some(ctx) => self.messages.get(&format!("{ctx}\u{4}{msgid}")),
            None => self.messages.get(msgid),
        }?;
        forms.first().map(String::as_str).filter(|s| !s.is_empty())
    }

    /// Форма по числу; `None` — строки нет, формы нет или она пуста.
    pub fn plural_lookup(&self, singular: &str, n: u64) -> Option<String> {
        let forms = self.messages.get(singular)?;
        forms.get(self.plural_index(n)).filter(|s| !s.is_empty()).cloned()
    }

    /// Номер формы по числу — по выражению `Plural-Forms` каталога.
    fn plural_index(&self, n: u64) -> usize {
        let index = match &self.plural {
            Some(expr) => expr.eval(n) as usize,
            // Заголовка нет — правило германских языков, как у gettext.
            None => usize::from(n != 1),
        };
        if self.nplurals > 0 { index.min(self.nplurals - 1) } else { index }
    }

    /// Перевод в смысле `QTranslator::translate`: то, что QML получает из
    /// `qsTranslate("rexon", source, disambiguation, n)`. `None` — перевода
    /// нет, и Qt покажет исходник (подставив `%n` сама).
    ///
    /// Правила Qt повторены там, где их ждёт автор QML:
    /// * уточнение = `msgctxt`; записи с таким уточнением нет — берётся
    ///   запись без него (у gettext такого отката нет, у Qt есть);
    /// * `n < 0` — «числа нет»: первая форма;
    /// * форма по числу — по `Plural-Forms` каталога, тем же правилом, что
    ///   у [`ntr`] в Rust, а не по своим таблицам Qt.
    ///
    /// Сверено 10.10.2026 с прежним каталогом `.qm` (тот же `.po` через
    /// `lconvert6` + `lrelease6`) на ru и de: все строки QML и весь шаблон
    /// с его собственными уточнениями, у строк с `%n` — 15 разных чисел,
    /// расхождений 0. Расходятся только запросы, которых нет ни в QML, ни
    /// в каталоге, и оба раза нарочно:
    /// * у записи одна форма (в `.po` нет `msgid_plural`), а число задано —
    ///   здесь перевод, у `.qm` был исходник (Qt искал форму с номером по
    ///   числу, а она у записи одна);
    /// * строка есть в каталоге только под ДРУГИМ уточнением — здесь
    ///   исходник, а `.qm` отдавал перевод той записи (`lrelease` пишет и
    ///   ключ без уточнения). Чужой смысл подсовывать нельзя; строки QML без
    ///   уточнения `xgettext` и так кладёт в каталог отдельной записью.
    pub fn qt_lookup(&self, disambiguation: Option<&str>, source: &str, n: i32) -> Option<String> {
        let with_context = disambiguation
            .filter(|d| !d.is_empty())
            .and_then(|d| self.messages.get(&format!("{d}\u{4}{source}")));
        let forms = with_context.or_else(|| self.messages.get(source))?;
        let form = match u64::try_from(n) {
            Ok(n) if forms.len() > 1 => forms.get(self.plural_index(n)),
            _ => forms.first(),
        };
        form.filter(|s| !s.is_empty()).cloned()
    }

    /// Все пары «оригинал → перевод» (для сверок каталогов).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.messages.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Сколько форм по числу у языка (из заголовка каталога).
    pub fn nplurals(&self) -> usize {
        self.nplurals
    }
}

/// Разобрать `Plural-Forms: nplurals=N; plural=ВЫРАЖЕНИЕ;` из заголовка.
fn parse_plural_header(header: &str) -> (usize, Option<PluralExpr>) {
    let Some(line) = header
        .lines()
        .find_map(|l| l.trim().strip_prefix("Plural-Forms:"))
    else {
        return (2, None);
    };
    let mut nplurals = 2;
    let mut expr = None;
    for part in line.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("nplurals=") {
            nplurals = v.trim().parse().unwrap_or(2);
        } else if let Some(v) = part.strip_prefix("plural=") {
            expr = PluralExpr::parse(v.trim()).ok();
        }
    }
    (nplurals, expr)
}

// ============================================================================
// Выражение Plural-Forms
// ============================================================================
//
// Это подмножество Си: `n`, целые, скобки, `?:`, `||`, `&&`, сравнения,
// `+ - * / %` и `!`. Разбор — рекурсивным спуском по приоритетам Си.

/// Разобранное выражение формы по числу.
#[derive(Debug, Clone, PartialEq)]
pub enum PluralExpr {
    N,
    Num(u64),
    Not(Box<PluralExpr>),
    Bin(Box<PluralExpr>, BinOp, Box<PluralExpr>),
    Cond(Box<PluralExpr>, Box<PluralExpr>, Box<PluralExpr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl PluralExpr {
    /// Разобрать выражение; хвостовая `;` допускается.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().trim_end_matches(';');
        let mut p = ExprParser { s: text.as_bytes(), i: 0 };
        let expr = p.ternary()?;
        p.skip_ws();
        if p.i != p.s.len() {
            return Err(format!("лишний хвост выражения на позиции {}", p.i));
        }
        Ok(expr)
    }

    /// Вычислить для `n`. Деление на ноль даёт 0, а не панику: выражение
    /// пришло из файла переводчика.
    pub fn eval(&self, n: u64) -> u64 {
        match self {
            PluralExpr::N => n,
            PluralExpr::Num(v) => *v,
            PluralExpr::Not(e) => u64::from(e.eval(n) == 0),
            PluralExpr::Cond(c, a, b) => {
                if c.eval(n) != 0 {
                    a.eval(n)
                } else {
                    b.eval(n)
                }
            }
            PluralExpr::Bin(a, op, b) => {
                let x = a.eval(n);
                // `||` и `&&` — с коротким замыканием, как в Си.
                match op {
                    BinOp::Or => return u64::from(x != 0 || b.eval(n) != 0),
                    BinOp::And => return u64::from(x != 0 && b.eval(n) != 0),
                    _ => {}
                }
                let y = b.eval(n);
                match op {
                    BinOp::Eq => u64::from(x == y),
                    BinOp::Ne => u64::from(x != y),
                    BinOp::Lt => u64::from(x < y),
                    BinOp::Le => u64::from(x <= y),
                    BinOp::Gt => u64::from(x > y),
                    BinOp::Ge => u64::from(x >= y),
                    BinOp::Add => x.wrapping_add(y),
                    BinOp::Sub => x.wrapping_sub(y),
                    BinOp::Mul => x.wrapping_mul(y),
                    BinOp::Div => x.checked_div(y).unwrap_or(0),
                    BinOp::Rem => x.checked_rem(y).unwrap_or(0),
                    BinOp::Or | BinOp::And => unreachable!(),
                }
            }
        }
    }
}

struct ExprParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl ExprParser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_ws();
        if self.s[self.i..].starts_with(token.as_bytes()) {
            self.i += token.len();
            true
        } else {
            false
        }
    }

    fn ternary(&mut self) -> Result<PluralExpr, String> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let a = self.ternary()?;
            if !self.eat(":") {
                return Err(format!("ждали «:» на позиции {}", self.i));
            }
            let b = self.ternary()?;
            return Ok(PluralExpr::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    /// Уровни приоритета Си снизу вверх. Двухзнаковые операторы стоят
    /// раньше своих однозначных префиксов (`<=` раньше `<`).
    const LEVELS: &'static [&'static [(&'static str, BinOp)]] = &[
        &[("||", BinOp::Or)],
        &[("&&", BinOp::And)],
        &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
        &[("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)],
        &[("+", BinOp::Add), ("-", BinOp::Sub)],
        &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Rem)],
    ];

    fn binary(&mut self, level: usize) -> Result<PluralExpr, String> {
        if level == Self::LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'outer: loop {
            for (token, op) in Self::LEVELS[level] {
                // `!=` не должен съесться как `!` уровнем ниже, а `|`/`&`
                // одиночных в выражении не бывает.
                if self.eat(token) {
                    let right = self.binary(level + 1)?;
                    left = PluralExpr::Bin(Box::new(left), *op, Box::new(right));
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Result<PluralExpr, String> {
        self.skip_ws();
        if self.s.get(self.i) == Some(&b'!') && self.s.get(self.i + 1) != Some(&b'=') {
            self.i += 1;
            return Ok(PluralExpr::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let e = self.ternary()?;
            if !self.eat(")") {
                return Err(format!("ждали «)» на позиции {}", self.i));
            }
            return Ok(e);
        }
        if self.eat("n") {
            return Ok(PluralExpr::N);
        }
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if start == self.i {
            return Err(format!("непонятный знак на позиции {start}"));
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .map(PluralExpr::Num)
            .ok_or_else(|| "слишком большое число".to_string())
    }
}

// ============================================================================
// Выбор языка
// ============================================================================

/// Коды языков для поиска каталога, по убыванию предпочтения.
///
/// `setting` — значение настройки (`None`/пусто/`"system"` — как в
/// системе), `env` — чтение переменной окружения (снаружи — чтобы сверки
/// не зависели от окружения прогона). Каждый код даёт себя и базовый язык:
/// `pt_BR.UTF-8` → `pt_BR`, `pt`. Английский (исходный) и `C`/`POSIX`
/// обрывают список: дальше искать незачем — строки и так английские.
pub fn language_candidates(
    setting: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    match setting.map(str::trim).filter(|s| !s.is_empty() && *s != "system") {
        Some(chosen) => raw.push(chosen.to_string()),
        None => {
            let nonempty = |k: &str| env(k).filter(|v| !v.trim().is_empty());
            let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| nonempty(k));
            // `LANGUAGE` — список через «:»; gettext не слушает его при
            // локали `C`, но мы слушаем: `LANGUAGE=en rexon` — ровно тот
            // способ проверить английский, которого ждёт разработчик.
            if let Some(list) = nonempty("LANGUAGE") {
                raw.extend(list.split(':').map(str::to_string));
            }
            raw.extend(locale);
        }
    }

    let mut out: Vec<String> = Vec::new();
    for item in raw {
        // `ru_RU.UTF-8@euro` → `ru_RU`.
        let code = item.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
        if code.is_empty() {
            continue;
        }
        if code == "C" || code == "POSIX" {
            break;
        }
        let base = code.split('_').next().unwrap_or(&code).to_string();
        if base == SOURCE_LANGUAGE {
            break;
        }
        for c in [code.clone(), base] {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// Каталоги переводов в порядке поиска.
pub fn locale_dirs(env: &dyn Fn(&str) -> Option<String>, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = env(ENV_LOCALEDIR).filter(|d| !d.trim().is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe) = exe_dir {
        dirs.push(exe.join("locale"));
    }
    dirs.push(PathBuf::from(SYSTEM_LOCALEDIR));
    dirs
}

/// Путь к файлу каталога: `<dir>/<язык>/LC_MESSAGES/rexon.<ext>`.
pub fn catalog_file(dir: &Path, language: &str, ext: &str) -> PathBuf {
    dir.join(language).join("LC_MESSAGES").join(format!("{DOMAIN}.{ext}"))
}

/// Первый язык из `candidates`, для которого нашёлся `.mo`, и путь к нему.
/// `None` — остаёмся на английском.
pub fn resolve(
    candidates: &[String],
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<(String, PathBuf)> {
    candidates.iter().find_map(|lang| {
        dirs.iter()
            .map(|d| catalog_file(d, lang, "mo"))
            .find(|p| exists(p))
            .map(|p| (lang.clone(), p))
    })
}

// ============================================================================
// Действующий каталог процесса
// ============================================================================

#[derive(Debug, Default)]
struct Active {
    /// Код языка; пусто — английский.
    language: String,
    mo: Option<PathBuf>,
    catalog: Option<Arc<Catalog>>,
}

fn state() -> &'static RwLock<Active> {
    static STATE: OnceLock<RwLock<Active>> = OnceLock::new();
    STATE.get_or_init(|| {
        // Ленивый выбор — только по окружению, БЕЗ чтения настроек:
        // сверки и мост в Python зовут `tr` без `init`, и читать им живой
        // профиль человека незачем. Настройку учитывает [`init`].
        //
        // Сверки самой библиотеки (`cargo test --lib`) начинают с ИСХОДНОГО
        // языка, а не с локали машины: иначе текстовая сверка краснела у
        // того, у кого `LANG=ru_RU` и стоит каталог (пакет в
        // /usr/share/locale или `REXON_LOCALEDIR`), и зеленела у остальных.
        // Сверки в `tests/` закрепляют язык сами (`tests/i18n_pin`).
        if cfg!(test) {
            return RwLock::new(Active::default());
        }
        RwLock::new(activate(None))
    })
}

/// Окружение процесса для выбора языка. Переменные локали — ИСХОДНЫЕ,
/// снятые до того, как окно подменило `LANGUAGE` ([`original_locale_env`]).
fn process_env(key: &str) -> Option<String> {
    if let Some((_, value)) = original_locale_env().iter().find(|(k, _)| *k == key) {
        return value.clone();
    }
    std::env::var(key).ok()
}

// ============================================================================
// Исходные переменные локали
// ============================================================================
//
// Живой случай 08.10.2026. Окно ставит `LANGUAGE=<язык Rexon>` (без этого
// стандартные кнопки Qt под Plasma говорят на языке системы), а выбор языка
// «как в системе» читал тот же `LANGUAGE` — уже подменённый. У владельца
// (система ru_RU, форматы LC_*=de_DE, выбран «de») пункт списка гласил
// «Wie im System (Deutsch)», а перезапуск после выбора «как в системе»
// возвращал немецкий: новый процесс наследовал подменённое окружение.
//
// Теперь исходные значения снимаются ОДИН раз, до подмены, и «системный»
// язык считается только по ним. Дочерним процессам Rexon исходный
// `LANGUAGE` едет переменной [`ORIGINAL_LANGUAGE_ENV`], а перезапуск
// возвращает окружение языка как было ([`restore_original_locale_env`]).
// Форматы (`LC_NUMERIC`, `LC_TIME`…) на язык интерфейса не влияют и в
// снимок не входят.

/// Переменные локали, по которым выбирается язык, — в порядке gettext.
pub const LOCALE_ENV_KEYS: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];

/// Исходный `LANGUAGE` процесса-родителя Rexon, подменившего свой: пусто —
/// переменной не было. Ставит [`override_ui_language_env`].
pub const ORIGINAL_LANGUAGE_ENV: &str = "REXON_ORIGINAL_LANGUAGE";

/// Снимок исходных переменных локали по окружению `env`. Если `env` несёт
/// [`ORIGINAL_LANGUAGE_ENV`] (процесс запущен Rexon'ом с подменённым
/// `LANGUAGE`), исходным считается он.
pub fn snapshot_locale_env(
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<(&'static str, Option<String>)> {
    LOCALE_ENV_KEYS
        .iter()
        .map(|&key| {
            let value = if key == "LANGUAGE" {
                match env(ORIGINAL_LANGUAGE_ENV) {
                    Some(orig) => Some(orig).filter(|v| !v.is_empty()),
                    None => env(key),
                }
            } else {
                env(key)
            };
            (key, value)
        })
        .collect()
}

/// Исходные переменные локали этого процесса (снимаются при первом
/// обращении — в окне это `i18n::init`, раньше подмены `LANGUAGE`).
pub fn original_locale_env() -> &'static [(&'static str, Option<String>)] {
    static ORIGINAL: OnceLock<Vec<(&'static str, Option<String>)>> = OnceLock::new();
    ORIGINAL.get_or_init(|| snapshot_locale_env(&|k| std::env::var(k).ok()))
}

/// Подменить `LANGUAGE` процесса языком Rexon `value`, запомнив исходный
/// для детей в [`ORIGINAL_LANGUAGE_ENV`].
///
/// ⚠️ Звать до создания потоков (меняет окружение процесса).
pub fn override_ui_language_env(value: &str) {
    let original = original_locale_env()
        .iter()
        .find(|(k, _)| *k == "LANGUAGE")
        .and_then(|(_, v)| v.clone())
        .unwrap_or_default();
    unsafe {
        std::env::set_var(ORIGINAL_LANGUAGE_ENV, original);
        std::env::set_var("LANGUAGE", value);
    }
}

/// Вернуть дочерней команде окружение языка, с которым запустили этот
/// процесс: новый Rexon (перезапуск) выбирает язык сам, по настройке и
/// ИСХОДНОЙ локали, и подменённый `LANGUAGE` ему только мешает.
pub fn restore_original_locale_env(cmd: &mut std::process::Command) {
    for (key, value) in original_locale_env() {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    cmd.env_remove(ORIGINAL_LANGUAGE_ENV);
}

/// Строка журнала о переменных языка: исходные и нынешние.
pub fn locale_env_report() -> String {
    let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_string());
    let orig: Vec<String> =
        original_locale_env().iter().map(|(k, v)| format!("{k}={}", show(v))).collect();
    let now: Vec<String> = LOCALE_ENV_KEYS
        .iter()
        .map(|k| format!("{k}={}", show(&std::env::var(k).ok())))
        .collect();
    format!("исходные [{}], сейчас [{}]", orig.join(" "), now.join(" "))
}

/// Каталог `.mo` действующего языка; `None` — английский.
pub fn mo_path() -> Option<PathBuf> {
    state().read().ok()?.mo.clone()
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

fn activate(setting: Option<&str>) -> Active {
    let candidates = language_candidates(setting, &process_env);
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    let Some((language, mo)) = resolve(&candidates, &dirs, &|p| p.is_file()) else {
        return Active::default();
    };
    match Catalog::load(&mo) {
        Ok(catalog) => Active { language, mo: Some(mo), catalog: Some(Arc::new(catalog)) },
        Err(e) => {
            crate::log_warn!(
                "lang",
                "каталог {} не прочитан, интерфейс будет на английском: {e}",
                mo.display()
            );
            Active::default()
        }
    }
}

fn catalog() -> Option<Arc<Catalog>> {
    state().read().ok()?.catalog.clone()
}

fn lookup(context: Option<&str>, msgid: &str) -> Option<String> {
    catalog()?.get(context, msgid).map(str::to_string)
}

/// Перевод строки QML действующим каталогом — дверь переводчика окна
/// (`rexon_gui::install_app_translator`). Правила — [`Catalog::qt_lookup`].
///
/// Зовётся из ЛЮБОГО потока: Qt переводит и в потоке загрузки QML. Каталог
/// читается под `RwLock`, без блокировки на запись, — поток GUI не ждёт.
pub fn qt_translate(disambiguation: Option<&str>, source: &str, n: i32) -> Option<String> {
    catalog()?.qt_lookup(disambiguation, source, n)
}

/// Выбрать язык с учётом настройки пользователя. Зовёт вход приложения
/// до первого окна. Возвращает код выбранного языка (`"en"` — исходный).
pub fn init() -> String {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    let setting = settings.get(SETTINGS_KEY).and_then(|v| v.as_str()).map(str::to_string);
    set_language(setting.as_deref())
}

/// Переключить язык: код (`"ru"`, `"pt_BR"`), `"en"` — исходный, `None`
/// или `"system"` — как в системе. Возвращает, что получилось.
///
/// Уже показанные строки сами не обновятся: Qt-стороне после этого надо
/// заново поставить переводчик окна (у него кэш строк), позвать
/// `retranslate()` движка и пересобрать модели страниц.
pub fn set_language(language: Option<&str>) -> String {
    let next = activate(language);
    let code = current_code(&next);
    if let Ok(mut guard) = state().write() {
        *guard = next;
    }
    code
}

fn current_code(active: &Active) -> String {
    if active.language.is_empty() {
        SOURCE_LANGUAGE.to_string()
    } else {
        active.language.clone()
    }
}

/// Код действующего языка (`"en"` — исходный, каталога нет).
pub fn current_language() -> String {
    state().read().map(|a| current_code(&a)).unwrap_or_else(|_| SOURCE_LANGUAGE.into())
}

// ============================================================================
// Выбор языка человеком: список, подписи, настройка
// ============================================================================
//
// Список языков в «Настройках» строится из `po/LINGUAS` — того же файла, по
// которому сборка собирает каталоги. Добавили строчку `pl` в LINGUAS и
// положили `po/pl.po` — польский появился в списке сам, код не трогается
// (правило #12). Показываются только языки, чей каталог действительно
// нашёлся: пункт, который ничего не переводит, был бы обманом.

/// Содержимое `po/LINGUAS` на момент сборки.
pub const LINGUAS: &str = include_str!("../../../po/LINGUAS");

/// Значение настройки «как в системе».
pub const SYSTEM_SETTING: &str = "system";

/// Коды языков из текста `LINGUAS`: через пробелы и переводы строк,
/// `#` — комментарий до конца строки (так же читает `scripts/i18n-update.sh`).
/// Повторы снимаются, порядок — как в файле.
pub fn parse_linguas(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        for code in line.split_whitespace() {
            if !out.iter().any(|c| c == code) {
                out.push(code.to_string());
            }
        }
    }
    out
}

/// Самоназвание языка — так, как его ищет в списке носитель, а не перевод
/// на язык интерфейса: человек, случайно включивший чужой язык, должен
/// найти свой, не читая по-чужому.
///
/// Таблица заранее покрывает все языки плана (решение владельца 29.09:
/// 36 языков), чтобы новый язык появлялся в списке без правки кода. Код,
/// которого здесь нет, подписывается самим кодом — видно, но не пусто.
pub fn native_name(code: &str) -> String {
    const NAMES: &[(&str, &str)] = &[
        ("en", "English"),
        ("ru", "Русский"),
        ("ar", "العربية"),
        ("be", "Беларуская"),
        ("bg", "Български"),
        ("cs", "Čeština"),
        ("da", "Dansk"),
        ("de", "Deutsch"),
        ("el", "Ελληνικά"),
        ("es", "Español"),
        ("fa", "فارسی"),
        ("fi", "Suomi"),
        ("fil", "Filipino"),
        ("fr", "Français"),
        ("he", "עברית"),
        ("hi", "हिन्दी"),
        ("hu", "Magyar"),
        ("hy", "Հայերեն"),
        ("id", "Bahasa Indonesia"),
        ("it", "Italiano"),
        ("ja", "日本語"),
        ("ka", "ქართული"),
        ("kk", "Қазақ тілі"),
        ("ko", "한국어"),
        ("ms", "Bahasa Melayu"),
        ("nb", "Norsk bokmål"),
        ("nl", "Nederlands"),
        ("pl", "Polski"),
        ("pt", "Português"),
        ("pt_BR", "Português (Brasil)"),
        ("ro", "Română"),
        ("sk", "Slovenčina"),
        ("sr", "Српски"),
        ("sv", "Svenska"),
        ("th", "ไทย"),
        ("tr", "Türkçe"),
        ("uk", "Українська"),
        ("vi", "Tiếng Việt"),
        ("zh_CN", "简体中文"),
    ];
    let code = code.trim();
    NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .or_else(|| {
            // `ru_RU` → «Русский»: страна к имени языка ничего не добавляет,
            // если отдельного перевода под неё нет.
            let base = code.split(['_', '-', '.', '@']).next().unwrap_or(code);
            NAMES.iter().find(|(c, _)| *c == base)
        })
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

/// Языки, которые можно выбрать: английский (исходный, каталог не нужен)
/// и каждый язык из `linguas`, для которого нашёлся `.mo`.
///
/// `exists` — снаружи, чтобы сверка не зависела от диска прогона.
pub fn selectable_languages(
    linguas: &[String],
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut out = vec![SOURCE_LANGUAGE.to_string()];
    for code in linguas {
        if code == SOURCE_LANGUAGE || out.contains(code) {
            continue;
        }
        if dirs.iter().any(|d| exists(&catalog_file(d, code, "mo"))) {
            out.push(code.clone());
        }
    }
    out
}

/// Языки для списка в «Настройках» на этой машине (см.
/// [`selectable_languages`]).
pub fn available_languages() -> Vec<String> {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    selectable_languages(&parse_linguas(LINGUAS), &dirs, &|p| p.is_file())
}

/// Перевод строки на каждый язык из `linguas`, чей `.mo` нашёлся в `dirs`,
/// — без смены языка процесса. Повторы снимаются, порядок — как в `linguas`.
///
/// ЗАЧЕМ. Кое-что Rexon пишет в ЧУЖИЕ файлы на языке интерфейса (название
/// правила KWin видно в «Правилах окон» KDE). Человек мог сменить язык после
/// записи, и найти свою прежнюю запись надо по любому из переводов, а не
/// только по нынешнему.
pub fn translations_in(msgid: &str, linguas: &[String], dirs: &[PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in linguas {
        let Some(mo) = dirs.iter().map(|d| catalog_file(d, code, "mo")).find(|p| p.is_file())
        else {
            continue;
        };
        let Ok(catalog) = Catalog::load(&mo) else { continue };
        if let Some(text) = catalog.get(None, msgid) {
            if !out.iter().any(|t| t == text) {
                out.push(text.to_string());
            }
        }
    }
    out
}

/// [`translations_in`] по всем языкам `po/LINGUAS`, установленным на этой
/// машине.
pub fn translations_everywhere(msgid: &str) -> Vec<String> {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    translations_in(msgid, &parse_linguas(LINGUAS), &dirs)
}

/// Какой язык получится при такой настройке — без загрузки каталога.
/// `None`/`"system"` — как в системе. Ответ — код (`"en"`, `"ru"`).
pub fn language_for_setting(setting: Option<&str>) -> String {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    resolve_setting(setting, &process_env, &dirs, &|p| p.is_file())
}

/// Чистая часть [`language_for_setting`]: окружение и диск — снаружи.
/// Язык без каталога даёт английский, а не пустоту.
pub fn resolve_setting(
    setting: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> String {
    resolve(&language_candidates(setting, env), dirs, exists)
        .map(|(lang, _)| lang)
        .unwrap_or_else(|| SOURCE_LANGUAGE.to_string())
}

/// Сохранённый выбор человека: `"system"`, `"en"`, `"ru"`… Пусто и
/// отсутствие ключа — `"system"`.
pub fn saved_setting() -> String {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    normalize_setting(settings.get(SETTINGS_KEY).and_then(|v| v.as_str()))
}

/// Привести значение настройки к виду списка: пусто → `"system"`.
pub fn normalize_setting(value: Option<&str>) -> String {
    match value.map(str::trim) {
        None | Some("") => SYSTEM_SETTING.to_string(),
        Some(v) => v.to_string(),
    }
}

/// Нужен ли перезапуск, чтобы сохранённый выбор вступил в силу: язык,
/// который он даст, не совпадает с языком этого процесса.
///
/// Язык на лету не меняется намеренно: строки вкладок собраны на Rust при
/// постройке моделей, и полупереведённое окно хуже честного «после
/// перезапуска». Процесс свой язык держит до выхода.
pub fn restart_needed(setting: &str) -> bool {
    language_for_setting(Some(setting)) != current_language()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собрать `.mo` в памяти — как `msgfmt`, без таблицы хэшей (она
    /// необязательна, и читатель её не использует).
    pub(crate) fn build_mo(entries: &[(&str, &str)], big_endian: bool) -> Vec<u8> {
        let mut sorted: Vec<_> = entries.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let n = sorted.len();
        let header_len = 28;
        let orig_table = header_len;
        let trans_table = orig_table + n * 8;
        let mut strings_at = trans_table + n * 8;
        let mut tables = Vec::new();
        let mut blob = Vec::new();
        let put = |v: u32, out: &mut Vec<u8>| {
            out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() })
        };
        let mut orig = Vec::new();
        let mut trans = Vec::new();
        for (o, _) in &sorted {
            orig.push((o.len(), strings_at));
            blob.extend_from_slice(o.as_bytes());
            blob.push(0);
            strings_at += o.len() + 1;
        }
        for (_, t) in &sorted {
            trans.push((t.len(), strings_at));
            blob.extend_from_slice(t.as_bytes());
            blob.push(0);
            strings_at += t.len() + 1;
        }
        let mut out = Vec::new();
        for v in [MO_MAGIC, 0, n as u32, orig_table as u32, trans_table as u32, 0, 0] {
            put(v, &mut out);
        }
        for (len, off) in orig.iter().chain(trans.iter()) {
            put(*len as u32, &mut tables);
            put(*off as u32, &mut tables);
        }
        out.extend(tables);
        out.extend(blob);
        out
    }

    const RU_PLURAL: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && \
                             n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";

    fn ru_catalog(big: bool) -> Catalog {
        let header = format!("Content-Type: text/plain; charset=UTF-8\nPlural-Forms: {RU_PLURAL}\n");
        let mo = build_mo(
            &[
                ("", &header),
                ("Next", "Далее"),
                ("verb\u{4}Open", "Открыть"),
                ("%n file\0%n files", "%n файл\0%n файла\0%n файлов"),
                ("Untranslated", ""),
            ],
            big,
        );
        Catalog::parse_mo(&mo).expect("каталог разбирается")
    }

    #[test]
    fn mo_reader_both_byte_orders() {
        for big in [false, true] {
            let c = ru_catalog(big);
            assert_eq!(c.get(None, "Next"), Some("Далее"));
            assert_eq!(c.get(Some("verb"), "Open"), Some("Открыть"));
            assert_eq!(c.get(None, "Open"), None, "контекст не теряется");
            assert_eq!(c.get(None, "Untranslated"), None, "пустой перевод — нет перевода");
            assert_eq!(c.nplurals(), 3);
        }
    }

    #[test]
    fn russian_plural_forms() {
        let c = ru_catalog(false);
        let got: Vec<_> = [0, 1, 2, 4, 5, 11, 12, 21, 22, 25, 101, 111]
            .iter()
            .map(|&n| c.plural_lookup("%n file", n).unwrap().replace("%n", &n.to_string()))
            .collect();
        assert_eq!(
            got,
            [
                "0 файлов", "1 файл", "2 файла", "4 файла", "5 файлов", "11 файлов",
                "12 файлов", "21 файл", "22 файла", "25 файлов", "101 файл", "111 файлов"
            ]
        );
    }

    /// Правила `qsTranslate` поверх `.mo` (сверка с прежним `.qm` — у
    /// [`Catalog::qt_lookup`]).
    #[test]
    fn qt_lookup_follows_qtranslator_rules() {
        let c = ru_catalog(false);
        let t = |d: Option<&str>, s: &str, n: i32| c.qt_lookup(d, s, n);
        assert_eq!(t(None, "Next", -1).as_deref(), Some("Далее"));
        // Пустое уточнение — то же, что его отсутствие.
        assert_eq!(t(Some(""), "Next", -1).as_deref(), Some("Далее"));
        assert_eq!(t(Some("verb"), "Open", -1).as_deref(), Some("Открыть"));
        // Записи без уточнения нет — запись под другим уточнением не
        // подставляется (прежний `.qm` подставлял, см. `qt_lookup`).
        assert_eq!(t(None, "Open", -1), None);
        // Чужое уточнение — откат к записи без него, как у QTranslator.
        assert_eq!(t(Some("no such"), "Next", -1).as_deref(), Some("Далее"));
        // У записи одна форма — она для любого числа.
        assert_eq!(t(None, "Next", 5).as_deref(), Some("Далее"));
        // Формы по числу — по Plural-Forms каталога; `%n` подставляет Qt.
        assert_eq!(t(None, "%n file", 1).as_deref(), Some("%n файл"));
        assert_eq!(t(None, "%n file", 3).as_deref(), Some("%n файла"));
        assert_eq!(t(None, "%n file", 11).as_deref(), Some("%n файлов"));
        assert_eq!(t(None, "%n file", -1).as_deref(), Some("%n файл"));
        // Пустой перевод и неизвестная строка — исходник (None).
        assert_eq!(t(None, "Untranslated", -1), None);
        assert_eq!(t(None, "Nowhere", -1), None);
    }

    #[test]
    fn broken_mo_is_an_error_not_a_panic() {
        assert!(Catalog::parse_mo(b"").is_err());
        assert!(Catalog::parse_mo(b"not a catalog at all, clearly").is_err());
        let mut mo = build_mo(&[("Next", "Далее")], false);
        mo.truncate(mo.len() - 4);
        assert!(Catalog::parse_mo(&mo).is_err());
    }

    #[test]
    fn plural_expressions_of_target_languages() {
        // Выражения — из заголовков каталогов этих языков у GNU gettext.
        let cases: &[(&str, &[(u64, u64)])] = &[
            ("n != 1", &[(0, 1), (1, 0), (2, 1)]),
            ("n > 1", &[(0, 0), (1, 0), (2, 1)]),
            ("(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2", &[(1, 0), (3, 1), (5, 2), (0, 2)]),
            (
                "(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2)",
                &[(1, 0), (2, 1), (5, 2), (12, 2), (22, 1), (112, 2)],
            ),
            ("0", &[(0, 0), (7, 0)]),
            ("!(n == 1)", &[(1, 0), (3, 1)]),
        ];
        for (text, expected) in cases {
            let e = PluralExpr::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
            for (n, want) in *expected {
                assert_eq!(e.eval(*n), *want, "{text} при n={n}");
            }
        }
        assert!(PluralExpr::parse("n %").is_err());
        assert!(PluralExpr::parse("(n == 1").is_err());
        assert_eq!(PluralExpr::parse("n / 0").unwrap().eval(3), 0);
    }

    #[test]
    fn fill_placeholders() {
        assert_eq!(fill("Step %1 of %2", &[&3, &8]), "Step 3 of 8");
        assert_eq!(fill("%2 — %1", &[&"a", &"b"]), "b — a");
        assert_eq!(fill("Шаг %1 из %2", &[&3, &8]), "Шаг 3 из 8");
        assert_eq!(fill("100% done, %3 left", &[&1]), "100% done, %3 left");
        assert_eq!(fill("%12", &(1..=12).map(|_| &0 as &dyn Display).collect::<Vec<_>>()), "0");
        assert_eq!(fill("", &[]), "");
    }

    #[test]
    fn language_candidates_from_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(language_candidates(None, &env(&[("LANG", "ru_RU.UTF-8")])), ["ru_RU", "ru"]);
        assert_eq!(
            language_candidates(None, &env(&[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "pt_BR.UTF-8")])),
            ["pt_BR", "pt"]
        );
        assert_eq!(
            language_candidates(None, &env(&[("LANGUAGE", "cs:pl"), ("LANG", "de_DE.UTF-8")])),
            ["cs", "pl", "de_DE", "de"]
        );
        // Английский и C обрывают поиск: `LANGUAGE=en` — проверка английского.
        assert!(language_candidates(None, &env(&[("LANGUAGE", "en"), ("LANG", "ru_RU.UTF-8")]))
            .is_empty());
        assert!(language_candidates(None, &env(&[("LANG", "C.UTF-8")])).is_empty());
        assert!(language_candidates(None, &env(&[])).is_empty());
        // Настройка сильнее окружения; «system» — значит окружение.
        assert_eq!(language_candidates(Some("pl"), &env(&[("LANG", "ru_RU.UTF-8")])), ["pl"]);
        assert_eq!(
            language_candidates(Some("system"), &env(&[("LANG", "ru_RU.UTF-8")])),
            ["ru_RU", "ru"]
        );
        assert!(language_candidates(Some("en"), &env(&[("LANG", "ru_RU.UTF-8")])).is_empty());
    }

    /// Живой случай 08.10.2026: система ru, форматы de, выбран de — окно
    /// подменило `LANGUAGE=de`, но «как в системе» обязано остаться русским.
    #[test]
    fn системный_язык_считается_по_исходной_локали() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        // Дочерний процесс Rexon: LANGUAGE подменён, исходный — в метке.
        let child = env(&[
            ("LANG", "ru_RU.UTF-8"),
            ("LANGUAGE", "de"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            (ORIGINAL_LANGUAGE_ENV, "ru"),
        ]);
        let snap = snapshot_locale_env(&child);
        let lookup = |k: &str| snap.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
        assert_eq!(lookup("LANGUAGE").as_deref(), Some("ru"));
        assert_eq!(language_candidates(None, &lookup), ["ru", "ru_RU"]);
        // Метка пустая — исходного LANGUAGE не было вовсе.
        let unset = env(&[("LANG", "ru_RU.UTF-8"), ("LANGUAGE", "de"), (ORIGINAL_LANGUAGE_ENV, "")]);
        let snap = snapshot_locale_env(&unset);
        let lookup = |k: &str| snap.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
        assert_eq!(lookup("LANGUAGE"), None);
        assert_eq!(language_candidates(None, &lookup), ["ru_RU", "ru"]);
        // Форматы (LC_NUMERIC) на язык не влияют.
        let formats = env(&[("LANG", "ru_RU.UTF-8"), ("LC_NUMERIC", "de_DE.UTF-8"), ("LC_TIME", "de_DE.UTF-8")]);
        assert_eq!(language_candidates(None, &formats), ["ru_RU", "ru"]);
        // Без метки (обычный запуск) — как есть.
        let plain = env(&[("LANGUAGE", "ru"), ("LANG", "ru_RU.UTF-8")]);
        let snap = snapshot_locale_env(&plain);
        assert_eq!(snap[0], ("LANGUAGE", Some("ru".to_string())));
    }

    #[test]
    fn resolve_takes_first_existing_catalog() {
        let dirs = vec![PathBuf::from("/dev-tree/locale"), PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| {
            p == Path::new("/usr/share/locale/pt/LC_MESSAGES/rexon.mo")
                || p == Path::new("/dev-tree/locale/ru/LC_MESSAGES/rexon.mo")
        };
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // `pt_BR` не нашёлся — берём базовый `pt`.
        assert_eq!(
            resolve(&c(&["pt_BR", "pt"]), &dirs, &have),
            Some(("pt".into(), PathBuf::from("/usr/share/locale/pt/LC_MESSAGES/rexon.mo")))
        );
        assert_eq!(
            resolve(&c(&["ru_RU", "ru"]), &dirs, &have).map(|(l, _)| l),
            Some("ru".into())
        );
        assert_eq!(resolve(&c(&["cs"]), &dirs, &have), None);
    }

    // ── Сверки самих каталогов po/ ───────────────────────────────────────

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Номера заполнителей `%1`, `%2`… строки (без `%n`).
    fn placeholders(text: &str) -> std::collections::BTreeSet<u32> {
        let b = text.as_bytes();
        let mut out = std::collections::BTreeSet::new();
        for i in 0..b.len() {
            if b[i] == b'%' {
                let digits: String =
                    b[i + 1..].iter().take(2).take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
                if let Ok(n) = digits.parse() {
                    out.insert(n);
                }
            }
        }
        out
    }

    /// Каждый язык из `po/LINGUAS` собирается настоящим `msgfmt --check`,
    /// читается нашим читателем, и в каждом переводе те же заполнители,
    /// что в оригинале: потерянный `%2` у переводчика дал бы строку без
    /// числа, и заметили бы это только глазами.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/*.po настоящим msgfmt")]
    fn every_catalog_compiles_and_keeps_placeholders() {
        let po = repo_root().join("po");
        let linguas = std::fs::read_to_string(po.join("LINGUAS")).expect("po/LINGUAS на месте");
        let langs: Vec<&str> = linguas
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .collect();
        assert!(!langs.is_empty(), "в po/LINGUAS ни одного языка");
        for lang in langs {
            let out = std::env::temp_dir().join(format!("rexon-i18n-{lang}-{}.mo", std::process::id()));
            let status = std::process::Command::new("msgfmt")
                .arg("--check")
                .arg("-o")
                .arg(&out)
                .arg(po.join(format!("{lang}.po")))
                .status()
                .expect("msgfmt (пакет gettext) запускается");
            assert!(status.success(), "msgfmt отверг po/{lang}.po");
            let catalog = Catalog::load(&out).expect("каталог читается");
            let _ = std::fs::remove_file(&out);
            for (key, forms) in catalog.entries() {
                let msgid = key.rsplit('\u{4}').next().unwrap_or(key);
                let want = placeholders(msgid);
                for form in forms.iter().filter(|f| !f.is_empty()) {
                    assert_eq!(placeholders(form), want, "{lang}: «{msgid}» → «{form}»");
                }
            }
        }
    }

    /// Шаблон `po/rexon.pot` не отстал от исходников: новая строка без
    /// обновления шаблона не дошла бы ни до одного переводчика.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: извлекает строки настоящим xgettext")]
    fn template_is_up_to_date() {
        let out = std::process::Command::new("bash")
            .arg(repo_root().join("scripts/i18n-update.sh"))
            .arg("--check")
            .output()
            .expect("bash запускается");
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn linguas_parsing_and_selectable_languages() {
        assert_eq!(parse_linguas("ru\n# comment\npl cs # tail\n\nru\n"), ["ru", "pl", "cs"]);
        // Настоящий LINGUAS читается и в нём есть русский.
        assert!(parse_linguas(LINGUAS).iter().any(|c| c == "ru"));

        let dirs = vec![PathBuf::from("/dev-tree/locale"), PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| p == Path::new("/usr/share/locale/ru/LC_MESSAGES/rexon.mo");
        let linguas: Vec<String> = ["ru", "pl", "en"].iter().map(|s| s.to_string()).collect();
        // Английский — всегда и первым; польский без каталога не показывается.
        assert_eq!(selectable_languages(&linguas, &dirs, &have), ["en", "ru"]);
        assert_eq!(selectable_languages(&[], &dirs, &have), ["en"]);
    }

    #[test]
    fn native_names() {
        assert_eq!(native_name("en"), "English");
        assert_eq!(native_name("ru"), "Русский");
        assert_eq!(native_name("ru_RU"), "Русский");
        assert_eq!(native_name("pt_BR"), "Português (Brasil)");
        assert_eq!(native_name("xx"), "xx");
        // Каждый язык плана подписан самоназванием, а не кодом.
        for code in [
            "ar", "zh_CN", "ja", "it", "es", "de", "fr", "pt_BR", "tr", "pl", "he", "cs", "vi",
            "th", "ko", "fil", "hi", "sv", "nb", "hy", "ka", "uk", "nl", "fi", "da", "hu", "ro",
            "be", "el", "bg", "id", "ms", "fa", "kk", "sk", "sr",
        ] {
            assert_ne!(native_name(code), code, "{code} без самоназвания");
        }
    }

    /// Три обещания выбора языка: `en` на русской системе — английский;
    /// «как в системе» — язык системы; язык без каталога — английский.
    #[test]
    fn setting_resolves_to_language() {
        let ru_system = |k: &str| (k == "LANG").then(|| "ru_RU.UTF-8".to_string());
        let dirs = vec![PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| p == Path::new("/usr/share/locale/ru/LC_MESSAGES/rexon.mo");
        assert_eq!(resolve_setting(Some("en"), &ru_system, &dirs, &have), "en");
        assert_eq!(resolve_setting(Some("system"), &ru_system, &dirs, &have), "ru");
        assert_eq!(resolve_setting(None, &ru_system, &dirs, &have), "ru");
        assert_eq!(resolve_setting(Some("ru"), &|_| None, &dirs, &have), "ru");
        assert_eq!(resolve_setting(Some("pl"), &ru_system, &dirs, &have), "en");
        let de_system = |k: &str| (k == "LANG").then(|| "de_DE.UTF-8".to_string());
        assert_eq!(resolve_setting(Some("system"), &de_system, &dirs, &have), "en");
    }

    #[test]
    fn setting_normalization() {
        assert_eq!(normalize_setting(None), "system");
        assert_eq!(normalize_setting(Some("  ")), "system");
        assert_eq!(normalize_setting(Some("ru")), "ru");
    }

    #[test]
    fn locale_dirs_order() {
        let env = |k: &str| (k == ENV_LOCALEDIR).then(|| "/tmp/po-build".to_string());
        assert_eq!(
            locale_dirs(&env, Some(Path::new("/opt/x"))),
            [
                PathBuf::from("/tmp/po-build"),
                PathBuf::from("/opt/x/locale"),
                PathBuf::from(SYSTEM_LOCALEDIR)
            ]
        );
    }
}
