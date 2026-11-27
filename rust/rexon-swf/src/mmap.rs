//! Frame writer for GNOME/Cinnamon (the shell extension reads the file, there is no window).
//!
//! The Rexon contract (`rexon-core::wallpaper_mmap`, commit 09081e3) that the writer follows but does NOT compute itself:
//! - the frame file is exactly width×height×4 bytes of RGBA8888 without a header; at start the previous file is unlinked
//!   and a fresh zero-length inode is opened — "the file grew to w·h·4" = "the first frame is ready";
//! - the generation counter is a separate file, u64 little-endian; 0 at start; odd before writing a frame,
//!   the next even value after (a seqlock for readers without flock);
//! - the frame write itself is under flock(LOCK_EX) of the frame file;
//! - the host takes the counter path (`frame_generation_path`) and the permissions (`FRAME_FILE_MODE`) from the core and passes them in [`MmapSpec`].

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::PathBuf;

/// Where and at what size to write frames.
#[derive(Debug, Clone)]
pub struct MmapSpec {
    /// Frame file (`REXON_WP_OUTPUT`).
    pub frame_path: PathBuf,
    /// Generation counter file; `None` — no counter (like a host of an older build).
    pub generation_path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    /// Permissions of created files (in Rexon — `wallpaper_mmap::FRAME_FILE_MODE` = 0o600).
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

    /// Current value of the generation counter (even between frames).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// How many frames have been written completely.
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

    /// Write an RGBA8888 frame with a dense layout (stride = width·4), exactly `frame_len()` bytes.
    pub fn write_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        if rgba.len() != self.frame_len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("frame is {} bytes, expected {}", rgba.len(), self.frame_len()),
            ));
        }
        flock(&self.frame, libc::LOCK_EX)?;
        let result = (|| {
            self.bump_generation()?; // odd: the frame is being written
            write_all_at(&self.frame, rgba, 0)
        })();
        // Even — even after a write error: otherwise readers would skip frames forever.
        let closed = if self.generation % 2 == 1 { self.bump_generation() } else { Ok(()) };
        let _ = flock(&self.frame, libc::LOCK_UN);
        result?;
        closed?;
        self.frames += 1;
        Ok(())
    }
}
