//! Random-access byte sources. `cos` never streams a PDF front to back: it
//! reads the header, the tail, and afterwards only the ranges the xref points
//! at. `CountingSource` makes that measurable.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};

pub trait Source: Send {
    fn len(&self) -> u64;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads up to `len` bytes at `offset`. A read that runs past the end of
    /// the source returns the bytes that exist, not an error: the parser grows
    /// its window until it either parses or reaches the end of the file.
    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>>;
}

pub struct BytesSource(Vec<u8>);

impl BytesSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        BytesSource(bytes)
    }
}

impl Source for BytesSource {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let start = offset.min(self.0.len() as u64) as usize;
        let end = start.saturating_add(len).min(self.0.len());
        Ok(self.0[start..end].to_vec())
    }
}

pub struct FileSource {
    file: Mutex<File>,
    len: u64,
}

impl FileSource {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(FileSource {
            file: Mutex::new(file),
            len,
        })
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset >= self.len {
            return Ok(Vec::new());
        }
        let want = len.min((self.len - offset) as usize);
        let mut buf = vec![0u8; want];
        let mut file = self
            .file
            .lock()
            .map_err(|_| Error::Io(io::Error::other("file source lock poisoned")))?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buf)?;
        Ok(buf)
    }
}

/// Wraps another source and counts the bytes actually handed back, which is
/// how the laziness budget (decision 11) is asserted in a test rather than
/// asserted in prose.
pub struct CountingSource {
    inner: Box<dyn Source>,
    read: Arc<AtomicU64>,
}

impl CountingSource {
    pub fn new(inner: Box<dyn Source>) -> (Self, Arc<AtomicU64>) {
        let read = Arc::new(AtomicU64::new(0));
        (
            CountingSource {
                inner,
                read: Arc::clone(&read),
            },
            read,
        )
    }
}

impl Source for CountingSource {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let bytes = self.inner.read_at(offset, len)?;
        self.read.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(bytes)
    }
}
