//! Library errors. No panics on environment errors: the host gets an enum and an exit code.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// No output was passed.
    NoOutputs,
    /// Output size is 0 or larger than 16384 on a side.
    InvalidSize { width: u32, height: u32 },
    /// Output index out of range (`resize`).
    OutputIndex(usize),
    /// The movie was not read or not parsed.
    MovieLoad(String),
    /// The wgpu surface from the raw handle was not created or is not supported by the adapter.
    Surface(String),
    /// No Vulkan adapter (for a window — one able to present to this surface).
    NoAdapter(String),
    /// The Ruffle render backend or the render target was not created.
    Render(String),
    /// The GPU stopped responding (poll/map readback).
    GpuLost(String),
    /// I/O error of the mmap frame or the generation counter.
    Output(std::io::Error),
}

impl Error {
    /// Exit code of the host process. 2 — call/arguments, 7 — GPU and surface, 8 — movie, 9 — frame file.
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
