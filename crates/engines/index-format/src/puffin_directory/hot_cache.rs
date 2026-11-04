// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment-open byte ranges stored inside the archive.
//!
//! Opening a Tantivy segment reads the footers of its composite files and, for each indexed
//! field, the term dictionary. Those reads are a few bytes each, scattered across the blobs, and
//! on an object store every one of them is a round trip. The writer opens the finished index
//! once, records the ranges Tantivy reads, and stores the small ones in the
//! [`FOOTER_CACHE_BLOB_TAG`](super::FOOTER_CACHE_BLOB_TAG) blob. The reader loads that blob with
//! the Puffin footer and answers those reads from memory.
//!
//! Encoding (little endian): `MSHC` | version `u32` | file count `u32`, then per file: path length
//! `u32` | path | range count `u32` | per range: start `u64` | length `u32` | bytes. Archives
//! written before this blob carried a placeholder, which [`HotCache::decode`] reads as empty.

use std::{
    io,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use bytes::Bytes;
use hashbrown::HashMap;
use tantivy::{
    HasLen, Index, ReloadPolicy,
    directory::{
        Directory, FileHandle, OwnedBytes, WatchCallback, WatchHandle, WritePtr,
        error::{DeleteError, LockError, OpenReadError, OpenWriteError},
    },
};

const MAGIC: &[u8; 4] = b"MSHC";
const VERSION: u32 = 1;

/// Longest single range kept. Composite-file footers are tens of bytes and the term dictionary of
/// a field with a few hundred values is a few hundred bytes; the dictionary of a full-text field
/// runs to tens of kilobytes per segment and stays on the object store, read by range when a
/// query needs it.
const MAX_RANGE_BYTES: usize = 8 * 1024;

/// Upper bound for one archive's cache; the smallest ranges are kept first. The footer cache
/// keeps one of these per archive, so it stays small.
const MAX_CACHE_BYTES: usize = 32 * 1024;

/// Every `(file, range)` Tantivy read while the writer opened the index.
type Reads = Arc<Mutex<Vec<(PathBuf, Range<usize>)>>>;

/// Recorded ranges per Tantivy file; ranges are sorted and do not overlap.
#[derive(Debug, Default, Clone)]
pub struct HotCache {
    files: HashMap<PathBuf, Vec<(u64, Bytes)>>,
}

impl HotCache {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn size_bytes(&self) -> usize {
        self.files
            .iter()
            .map(|(path, ranges)| {
                path.as_os_str().len()
                    + ranges
                        .iter()
                        .map(|(_, bytes)| bytes.len() + 16)
                        .sum::<usize>()
            })
            .sum()
    }

    /// The bytes of `range` in `path` when a recorded range covers all of it.
    pub fn get(&self, path: &Path, range: &Range<usize>) -> Option<Bytes> {
        let ranges = self.files.get(path)?;
        let start = range.start as u64;
        let end = range.end as u64;

        let index = ranges.partition_point(|(offset, _)| *offset <= start);
        let (offset, bytes) = ranges.get(index.checked_sub(1)?)?;

        if end > offset + bytes.len() as u64 {
            return None;
        }

        Some(bytes.slice((start - offset) as usize..(end - offset) as usize))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size_bytes() + 16);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.files.len() as u32).to_le_bytes());

        let mut paths: Vec<&PathBuf> = self.files.keys().collect();
        paths.sort();

        for path in paths {
            let name = path.to_string_lossy();
            out.extend_from_slice(&(name.len() as u32).to_le_bytes());
            out.extend_from_slice(name.as_bytes());

            let ranges = &self.files[path];
            out.extend_from_slice(&(ranges.len() as u32).to_le_bytes());

            for (start, bytes) in ranges {
                out.extend_from_slice(&start.to_le_bytes());
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(bytes);
            }
        }

        out
    }

    /// Empty for anything that is not an encoded cache, such as the placeholder older archives
    /// carry; a cache that does not decode only costs the reads it would have saved.
    pub fn decode(blob: &Bytes) -> Self {
        Self::try_decode(blob).unwrap_or_default()
    }

    fn try_decode(blob: &Bytes) -> Option<Self> {
        let mut cursor = Cursor { blob, at: 0 };

        if cursor.take(4)? != MAGIC.as_slice() || cursor.u32()? != VERSION {
            return None;
        }

        let file_count = cursor.u32()?;
        let mut files = HashMap::with_capacity(file_count as usize);

        for _ in 0..file_count {
            let name_len = cursor.u32()? as usize;
            let name = std::str::from_utf8(&cursor.take(name_len)?)
                .ok()?
                .to_owned();
            let range_count = cursor.u32()?;
            let mut ranges = Vec::with_capacity(range_count as usize);

            for _ in 0..range_count {
                let start = cursor.u64()?;
                let len = cursor.u32()? as usize;
                ranges.push((start, cursor.take(len)?));
            }

            files.insert(PathBuf::from(name), ranges);
        }

        Some(Self { files })
    }

    /// Keeps the smallest recorded reads within the size limits and merges overlapping ones.
    fn from_reads(reads: Vec<(PathBuf, Range<usize>)>, files: &dyn Directory) -> Result<Self> {
        let mut by_file: HashMap<PathBuf, Vec<Range<usize>>> = HashMap::new();

        for (path, range) in reads {
            if !range.is_empty() && range.len() <= MAX_RANGE_BYTES {
                by_file.entry(path).or_default().push(range);
            }
        }

        let mut merged: Vec<(PathBuf, Range<usize>)> = Vec::new();

        for (path, mut ranges) in by_file {
            ranges.sort_by_key(|range| range.start);
            let mut current: Option<Range<usize>> = None;

            for range in ranges {
                current = match current {
                    Some(open) if range.start <= open.end => {
                        Some(open.start..open.end.max(range.end))
                    }
                    Some(open) => {
                        merged.push((path.clone(), open));
                        Some(range)
                    }
                    None => Some(range),
                };
            }

            if let Some(open) = current {
                merged.push((path, open));
            }
        }

        merged.sort_by_key(|(_, range)| range.len());

        let mut kept = 0;
        let mut files_out: HashMap<PathBuf, Vec<(u64, Bytes)>> = HashMap::new();

        for (path, range) in merged {
            if kept + range.len() > MAX_CACHE_BYTES {
                break;
            }

            let handle = files
                .get_file_handle(&path)
                .map_err(|error| anyhow!("hot cache open {path:?}: {error}"))?;

            let bytes = handle
                .read_bytes(range.clone())
                .map_err(|error| anyhow!("hot cache read {path:?}: {error}"))?;

            kept += range.len();
            files_out
                .entry(path)
                .or_default()
                .push((range.start as u64, Bytes::copy_from_slice(bytes.as_slice())));
        }

        for ranges in files_out.values_mut() {
            ranges.sort_by_key(|(start, _)| *start);
        }

        Ok(Self { files: files_out })
    }
}

/// Opens the committed index in `files`, records what segment opening reads, and returns the
/// part worth storing.
pub(super) fn build(files: Arc<dyn Directory>) -> Result<HotCache> {
    let reads: Reads = Arc::new(Mutex::new(Vec::new()));

    let recording = RecordingDirectory {
        inner: files.clone(),
        reads: reads.clone(),
    };

    let index = Index::open(recording).map_err(|error| anyhow!("hot cache open index: {error}"))?;

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .map_err(|error| anyhow!("hot cache reader: {error}"))?;

    let searcher = reader.searcher();
    let schema = index.schema();

    for segment in searcher.segment_readers() {
        for (field, entry) in schema.fields() {
            if entry.is_indexed() {
                segment
                    .inverted_index(field)
                    .map_err(|error| anyhow!("hot cache term dictionary: {error}"))?;
            }
        }
    }

    drop(searcher);
    drop(reader);
    drop(index);

    let reads = std::mem::take(&mut *reads.lock().expect("hot cache reads poisoned"));
    HotCache::from_reads(reads, files.as_ref())
}

struct Cursor<'a> {
    blob: &'a Bytes,
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, len: usize) -> Option<Bytes> {
        let end = self.at.checked_add(len)?;

        if end > self.blob.len() {
            return None;
        }

        let bytes = self.blob.slice(self.at..end);
        self.at = end;
        Some(bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.as_ref().try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.as_ref().try_into().ok()?))
    }
}

/// Read-only view that notes every byte range Tantivy reads.
#[derive(Clone, Debug)]
struct RecordingDirectory {
    inner: Arc<dyn Directory>,
    reads: Reads,
}

#[derive(Debug)]
struct RecordingHandle {
    path: PathBuf,
    inner: Arc<dyn FileHandle>,
    reads: Reads,
}

impl HasLen for RecordingHandle {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

#[async_trait::async_trait]
impl FileHandle for RecordingHandle {
    fn read_bytes(&self, range: Range<usize>) -> io::Result<OwnedBytes> {
        self.reads
            .lock()
            .expect("hot cache reads poisoned")
            .push((self.path.clone(), range.clone()));

        self.inner.read_bytes(range)
    }
}

impl Directory for RecordingDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        Ok(Arc::new(RecordingHandle {
            path: path.to_path_buf(),
            inner: self.inner.get_file_handle(path)?,
            reads: self.reads.clone(),
        }))
    }

    fn delete(&self, _path: &Path) -> Result<(), DeleteError> {
        unimplemented!("read-only recording directory")
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        self.inner.exists(path)
    }

    fn open_write(&self, _path: &Path) -> Result<WritePtr, OpenWriteError> {
        unimplemented!("read-only recording directory")
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        self.inner.atomic_read(path)
    }

    fn atomic_write(&self, _path: &Path, _data: &[u8]) -> io::Result<()> {
        unimplemented!("read-only recording directory")
    }

    fn sync_directory(&self) -> io::Result<()> {
        Ok(())
    }

    fn watch(&self, _callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        Ok(WatchHandle::empty())
    }

    fn acquire_lock(
        &self,
        _lock: &tantivy::directory::Lock,
    ) -> Result<tantivy::directory::DirectoryLock, LockError> {
        Ok(tantivy::directory::DirectoryLock::from(Box::new(())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(ranges: &[(&str, u64, &[u8])]) -> HotCache {
        let mut files: HashMap<PathBuf, Vec<(u64, Bytes)>> = HashMap::new();

        for (path, start, bytes) in ranges {
            files
                .entry(PathBuf::from(path))
                .or_default()
                .push((*start, Bytes::copy_from_slice(bytes)));
        }

        HotCache { files }
    }

    #[test]
    fn serves_only_ranges_it_fully_covers() {
        let hot = cache(&[("a.term", 10, b"abcdef"), ("a.term", 40, b"xyz")]);

        assert_eq!(
            hot.get(Path::new("a.term"), &(11..14)).unwrap().as_ref(),
            b"bcd"
        );
        assert_eq!(
            hot.get(Path::new("a.term"), &(40..43)).unwrap().as_ref(),
            b"xyz"
        );
        assert!(hot.get(Path::new("a.term"), &(14..17)).is_none());
        assert!(hot.get(Path::new("a.term"), &(5..12)).is_none());
        assert!(hot.get(Path::new("b.term"), &(10..12)).is_none());
    }

    #[test]
    fn round_trips_through_its_encoding() {
        let hot = cache(&[("a.term", 10, b"abcdef"), ("b.idx", 0, b"\x00\x01")]);
        let decoded = HotCache::decode(&Bytes::from(hot.encode()));

        assert_eq!(
            decoded
                .get(Path::new("a.term"), &(10..16))
                .unwrap()
                .as_ref(),
            b"abcdef"
        );
        assert_eq!(
            decoded.get(Path::new("b.idx"), &(0..2)).unwrap().as_ref(),
            b"\x00\x01"
        );
    }

    #[test]
    fn placeholder_and_truncated_blobs_decode_as_empty() {
        assert!(HotCache::decode(&Bytes::from_static(b"__placeholder_footer_cache__")).is_empty());

        let encoded = cache(&[("a.term", 10, b"abcdef")]).encode();
        assert!(
            HotCache::decode(&Bytes::copy_from_slice(&encoded[..encoded.len() - 1])).is_empty()
        );
    }
}
