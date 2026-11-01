//! Ошибки библиотеки. Никаких panic на ошибках окружения: хост получает перечисление и код выхода.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Не передано ни одного вывода.
    NoOutputs,
    /// Размер вывода 0 или больше 16384 по стороне.
    InvalidSize { width: u32, height: u32 },
    /// Номер вывода вне диапазона (`resize`).
    OutputIndex(usize),
    /// Ролик не прочитан или не разобран.
    MovieLoad(String),
    /// wgpu-поверхность из raw-handle не создана или не поддерживается адаптером.
    Surface(String),
    /// Нет Vulkan-адаптера (для окна — умеющего показывать в эту поверхность).
    NoAdapter(String),
    /// Рендер-бэкенд Ruffle или цель рендера не создались.
    Render(String),
    /// GPU перестал отвечать (poll/map readback).
    GpuLost(String),
    /// Ошибка ввода-вывода mmap-кадра или счётчика поколения.
    Output(std::io::Error),
}

impl Error {
    /// Код выхода процесса хоста. 2 — вызов/аргументы, 7 — GPU и поверхность, 8 — ролик, 9 — файл кадра.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::NoOutputs | Error::InvalidSize { .. } | Error::OutputIndex(_) => 2,
            Error::Surface(_) | Error::NoAdapter(_) | Error::Render(_) | Error::GpuLost(_) => 7,
            Error::MovieLoad(_) => 8,
            Error::Output(_) => 9,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoOutputs => write!(f, "no outputs"),
            Error::InvalidSize { width, height } => write!(f, "invalid output size {width}x{height}"),
            Error::OutputIndex(i) => write!(f, "no output #{i}"),
            Error::MovieLoad(e) => write!(f, "movie: {e}"),
            Error::Surface(e) => write!(f, "surface: {e}"),
            Error::NoAdapter(e) => write!(f, "no Vulkan adapter: {e}"),
            Error::Render(e) => write!(f, "render backend: {e}"),
            Error::GpuLost(e) => write!(f, "GPU: {e}"),
            Error::Output(e) => write!(f, "frame file: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Output(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Output(e)
    }
}
