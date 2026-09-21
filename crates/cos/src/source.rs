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

pub struct BytesSource {
    bytes: Arc<Vec<u8>>,
    /// How much of `bytes` the source is: all of it, or a prefix.
    len: usize,
}

impl BytesSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        BytesSource::from_shared(Arc::new(bytes))
    }

    /// Reads from a buffer somebody else also holds.
    ///
    /// Consumer: the viewer session, which reads a file once and hands the same
    /// bytes to `cos` and to hayro (whose `PdfData` is `From<Arc<T>>`). Without
    /// this the two parsers would each own a copy of the file.
    pub fn from_shared(bytes: Arc<Vec<u8>>) -> Self {
        let len = bytes.len();
        BytesSource { bytes, len }
    }

    /// The first `len` bytes of a shared buffer, without copying them: the
    /// file as it was when an earlier generation ended.
    ///
    /// Consumer: the skins panel, which opens each generation of a file to
    /// read its trailer, and would otherwise copy the file once per
    /// generation.
    pub fn prefix(bytes: Arc<Vec<u8>>, len: usize) -> Self {
        let len = len.min(bytes.len());
        BytesSource { bytes, len }
    }
}

impl Source for BytesSource {
    fn len(&self) -> u64 {
        self.len as u64
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let start = offset.min(self.len as u64) as usize;
        let end = start.saturating_add(len).min(self.len);
        Ok(self.bytes[start..end].to_vec())
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

/// What a `CountingSource` observed.
///
/// This exists to be asserted on. `cos` itself never reads it: the laziness
/// budget (decision 11) and the streaming save are claims about how much of a
/// file gets read, and the guarantee tests are the consumer that turns them
/// into numbers instead of prose.
#[derive(Default)]
pub struct ReadStats {
    total: AtomicU64,
    largest: AtomicU64,
}

impl ReadStats {
    /// Bytes handed back across every read.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// The biggest single read. What a caller held in memory at once is at
    /// least this, so a bound on it is a bound on the caller's buffering.
    pub fn largest_read(&self) -> u64 {
        self.largest.load(Ordering::Relaxed)
    }

    /// Clears both counters so one phase of a session can be measured without
    /// the previous phase's reads in the numbers: the streaming-save test
    /// separates what the save reads from what the open read.
    pub fn reset(&self) {
        self.total.store(0, Ordering::Relaxed);
        self.largest.store(0, Ordering::Relaxed);
    }

    fn record(&self, bytes: u64) {
        self.total.fetch_add(bytes, Ordering::Relaxed);
        self.largest.fetch_max(bytes, Ordering::Relaxed);
    }
}

/// Wraps another source and counts the bytes actually handed back, which is
/// how the laziness budget (decision 11) is asserted in a test rather than
/// asserted in prose.
pub struct CountingSource {
    inner: Box<dyn Source>,
    stats: Arc<ReadStats>,
}

impl CountingSource {
    pub fn new(inner: Box<dyn Source>) -> (Self, Arc<ReadStats>) {
        let stats = Arc::new(ReadStats::default());
        (
            CountingSource {
                inner,
                stats: Arc::clone(&stats),
            },
            stats,
        )
    }
}

impl Source for CountingSource {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let bytes = self.inner.read_at(offset, len)?;
        self.stats.record(bytes.len() as u64);
        Ok(bytes)
    }
}
