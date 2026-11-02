//! Positioned reads from the bytes of an HDF5 file, so the reader never loads the whole file.

use std::fs::File;
use std::io;

/// Random-access byte source. Reads take `&self`, so one source can serve several threads.
pub(crate) trait ReadAt: Send + Sync {
    /// Total size in bytes.
    fn len(&self) -> u64;

    /// Fills `buf` with the bytes starting at `offset`, or fails if they are not all there.
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
}

/// A file on disk, read with the platform's positioned-read call (no shared cursor, no lock).
pub(crate) struct FileSource {
    file: File,
    len: u64,
}

impl FileSource {
    pub(crate) fn new(file: File) -> io::Result<Self> {
        let len = file.metadata()?.len();
        Ok(FileSource { file, len })
    }
}

impl ReadAt for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(&self.file, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match self.file.seek_read(buf, offset) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn read_exact_at(&self, _buf: &mut [u8], _offset: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Bytes already in memory (unit tests build synthetic files with it).
#[cfg(test)]
pub(crate) struct MemorySource(pub Vec<u8>);

#[cfg(test)]
impl ReadAt for MemorySource {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        let src = start
            .checked_add(buf.len())
            .and_then(|end| self.0.get(start..end))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}
