//! Писатель кадра для GNOME/Cinnamon (расширение оболочки читает файл, окна нет).
//!
//! Договор Rexon (`rexon-core::wallpaper_mmap`, коммит 09081e3), который писатель соблюдает, но НЕ вычисляет сам:
//! - файл кадра — ровно width×height×4 байт RGBA8888 без заголовка; при старте прежний файл отцепляется
//!   (unlink) и открывается свежий inode нулевой длины — «файл дорос до w·h·4» = «первый кадр готов»;
//! - счётчик поколения — отдельный файл, u64 little-endian; при старте 0; перед записью кадра нечётное,
//!   после — следующее чётное (seqlock для читателей без flock);
//! - сама запись кадра — под flock(LOCK_EX) файла кадра;
//! - путь счётчика (`frame_generation_path`) и права (`FRAME_FILE_MODE`) хост берёт из ядра и передаёт в [`MmapSpec`].

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::PathBuf;

/// Куда и какого размера писать кадры.
#[derive(Debug, Clone)]
pub struct MmapSpec {
    /// Файл кадра (`REXON_WP_OUTPUT`).
    pub frame_path: PathBuf,
    /// Файл счётчика поколения; `None` — без счётчика (как хост прежней сборки).
    pub generation_path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    /// Права создаваемых файлов (в Rexon — `wallpaper_mmap::FRAME_FILE_MODE` = 0o600).
    pub file_mode: u32,
}

#[derive(Debug)]
pub struct MmapWriter {
    frame: File,
    generation_file: Option<File>,
    width: u32,
    height: u32,
    generation: u64,
    frames: u64,
}

fn open_fresh(path: &PathBuf, mode: u32) -> io::Result<File> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(true).mode(mode).open(path)
}

fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    while !buf.is_empty() {
        match file.write_at(buf, offset) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0")),
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn flock(file: &File, op: libc::c_int) -> io::Result<()> {
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), op) } == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

impl MmapWriter {
    pub fn create(spec: &MmapSpec) -> io::Result<Self> {
        if spec.width == 0 || spec.height == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "zero frame size"));
        }
        let frame = open_fresh(&spec.frame_path, spec.file_mode)?;
        let generation_file = match &spec.generation_path {
            Some(p) => {
                let f = open_fresh(p, spec.file_mode)?;
                write_all_at(&f, &0u64.to_le_bytes(), 0)?;
                Some(f)
            }
            None => None,
        };
        Ok(Self { frame, generation_file, width: spec.width, height: spec.height, generation: 0, frames: 0 })
    }

    pub fn frame_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Текущее значение счётчика поколения (чётное между кадрами).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Сколько кадров записано целиком.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    fn bump_generation(&mut self) -> io::Result<()> {
        self.generation += 1;
        match &self.generation_file {
            Some(f) => write_all_at(f, &self.generation.to_le_bytes(), 0),
            None => Ok(()),
        }
    }

    /// Записать кадр RGBA8888 плотной раскладки (stride = width·4), ровно `frame_len()` байт.
    pub fn write_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        if rgba.len() != self.frame_len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("frame is {} bytes, expected {}", rgba.len(), self.frame_len()),
            ));
        }
        flock(&self.frame, libc::LOCK_EX)?;
        let result = (|| {
            self.bump_generation()?; // нечётное: кадр пишется
            write_all_at(&self.frame, rgba, 0)
        })();
        // Чётное — даже после ошибки записи: иначе читатели пропускали бы кадры навсегда.
        let closed = if self.generation % 2 == 1 { self.bump_generation() } else { Ok(()) };
        let _ = flock(&self.frame, libc::LOCK_UN);
        result?;
        closed?;
        self.frames += 1;
        Ok(())
    }
}
