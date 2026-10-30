//! Generation-based redb-backed Tantivy directory.
//!
//! Segment bytes are immutable blobs. A staged generation owns only path-to-blob
//! references, and becomes visible when the integrated coordinator changes the
//! active generation pointer in its final redb transaction.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, BufWriter, Cursor, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use sha2::{Digest, Sha256};
use tantivy::directory::error::{DeleteError, LockError, OpenReadError, OpenWriteError};
use tantivy::directory::{
    AntiCallToken, Directory, DirectoryLock, FileHandle, FileSlice, Lock, TerminatingWrite,
    WatchCallback, WatchCallbackList, WatchHandle, WritePtr,
};

const FULLTEXT_BLOBS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("fulltext_blobs");
const FULLTEXT_GENERATION_FILES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("fulltext_generation_files");
const FULLTEXT_ACTIVE_GENERATIONS: TableDefinition<&[u8], u64> =
    TableDefinition::new("fulltext_active_generations");
const FULLTEXT_GENERATION_COUNTERS: TableDefinition<&[u8], u64> =
    TableDefinition::new("fulltext_generation_counters");
const FULLTEXT_DROPPED_NAMESPACES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("fulltext_dropped_namespaces");

fn io_error(error: impl fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

/// A prepared generation that can be published by a coordinator-owned transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FulltextGeneration {
    namespace: Arc<[u8]>,
    generation: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FulltextGcReport {
    pub generation_files_removed: usize,
    pub blobs_removed: usize,
}

impl FulltextGeneration {
    pub fn namespace(&self) -> &[u8] {
        &self.namespace
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Change only the visibility pointer. Segment blobs were staged beforehand.
    pub fn activate(&self, transaction: &WriteTransaction) -> io::Result<()> {
        let mut active = transaction
            .open_table(FULLTEXT_ACTIVE_GENERATIONS)
            .map_err(io_error)?;
        active
            .insert(self.namespace.as_ref(), self.generation)
            .map_err(io_error)?;
        drop(active);
        transaction
            .open_table(FULLTEXT_DROPPED_NAMESPACES)
            .map_err(io_error)?
            .remove(self.namespace.as_ref())
            .map_err(io_error)?;
        Ok(())
    }
}

/// A Tantivy virtual directory bound to one immutable generation namespace.
#[derive(Clone)]
pub struct RedbDirectory {
    db: Arc<Database>,
    namespace: Arc<[u8]>,
    generation: u64,
    watch_callbacks: Arc<WatchCallbackList>,
    locks: Arc<RedbLockState>,
}

#[derive(Default)]
struct RedbLockState {
    held: Mutex<HashSet<PathBuf>>,
    available: Condvar,
}

struct RedbLockGuard {
    state: Arc<RedbLockState>,
    path: PathBuf,
}

impl Drop for RedbLockGuard {
    fn drop(&mut self) {
        self.state.held.lock().remove(&self.path);
        self.state.available.notify_all();
    }
}

impl fmt::Debug for RedbDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RedbDirectory")
            .field("namespace", &String::from_utf8_lossy(&self.namespace))
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl RedbDirectory {
    /// Return the currently visible generation without opening a Tantivy directory.
    pub fn active_generation(
        db: &Database,
        namespace: impl AsRef<[u8]>,
    ) -> io::Result<Option<u64>> {
        let namespace = validate_namespace(namespace.as_ref())?;
        active_generation(db, &namespace)
    }

    /// Remove reader visibility for a namespace inside a coordinator-owned transaction.
    pub fn deactivate(
        transaction: &WriteTransaction,
        namespace: impl AsRef<[u8]>,
    ) -> io::Result<bool> {
        let namespace = validate_namespace(namespace.as_ref())?;
        let removed = transaction
            .open_table(FULLTEXT_ACTIVE_GENERATIONS)
            .map_err(io_error)?
            .remove(namespace.as_ref())
            .map_err(io_error)?
            .is_some();
        if removed {
            let marker: &[u8] = &[];
            transaction
                .open_table(FULLTEXT_DROPPED_NAMESPACES)
                .map_err(io_error)?
                .insert(namespace.as_ref(), marker)
                .map_err(io_error)?;
        }
        Ok(removed)
    }

    /// Create an empty, invisible generation for a new Tantivy index.
    pub fn stage_empty(db: Arc<Database>, namespace: impl AsRef<[u8]>) -> io::Result<Self> {
        let namespace = validate_namespace(namespace.as_ref())?;
        let generation = allocate_generation(&db, &namespace, None)?;
        Ok(Self::from_parts(db, namespace, generation))
    }

    /// Copy the active path manifest into a new invisible generation.
    pub fn stage_from_active(db: Arc<Database>, namespace: impl AsRef<[u8]>) -> io::Result<Self> {
        let namespace = validate_namespace(namespace.as_ref())?;
        let active = active_generation(&db, &namespace)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "fulltext namespace '{}' has no active generation",
                    String::from_utf8_lossy(&namespace)
                ),
            )
        })?;
        let generation = allocate_generation(&db, &namespace, Some(active))?;
        Ok(Self::from_parts(db, namespace, generation))
    }

    /// Open the generation currently visible to readers.
    pub fn open_active(db: Arc<Database>, namespace: impl AsRef<[u8]>) -> io::Result<Self> {
        let namespace = validate_namespace(namespace.as_ref())?;
        let generation = active_generation(&db, &namespace)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "fulltext namespace '{}' has no active generation",
                    String::from_utf8_lossy(&namespace)
                ),
            )
        })?;
        Ok(Self::from_parts(db, namespace, generation))
    }

    fn from_parts(db: Arc<Database>, namespace: Arc<[u8]>, generation: u64) -> Self {
        Self {
            db,
            namespace,
            generation,
            watch_callbacks: Arc::new(WatchCallbackList::default()),
            locks: Arc::new(RedbLockState::default()),
        }
    }

    pub fn staged_generation(&self) -> FulltextGeneration {
        FulltextGeneration {
            namespace: self.namespace.clone(),
            generation: self.generation,
        }
    }

    /// Standalone publication used by non-integrated callers and creation APIs.
    pub fn activate_standalone(&self) -> io::Result<()> {
        let transaction = self.db.begin_write().map_err(io_error)?;
        self.staged_generation().activate(&transaction)?;
        transaction.commit().map_err(io_error)
    }

    fn key(&self, path: &Path) -> io::Result<Vec<u8>> {
        generation_file_key(&self.namespace, self.generation, path)
    }

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let key = self
            .key(path)
            .map_err(|error| OpenReadError::wrap_io_error(error, path.to_path_buf()))?;
        let read = self
            .db
            .begin_read()
            .map_err(|error| OpenReadError::wrap_io_error(io_error(error), path.to_path_buf()))?;
        let files = read
            .open_table(FULLTEXT_GENERATION_FILES)
            .map_err(|error| OpenReadError::wrap_io_error(io_error(error), path.to_path_buf()))?;
        let blob_key = files
            .get(key.as_slice())
            .map_err(|error| OpenReadError::wrap_io_error(io_error(error), path.to_path_buf()))?
            .map(|value| value.value().to_vec())
            .ok_or_else(|| OpenReadError::FileDoesNotExist(path.to_path_buf()))?;
        let blobs = read
            .open_table(FULLTEXT_BLOBS)
            .map_err(|error| OpenReadError::wrap_io_error(io_error(error), path.to_path_buf()))?;
        blobs
            .get(blob_key.as_slice())
            .map_err(|error| OpenReadError::wrap_io_error(io_error(error), path.to_path_buf()))?
            .map(|value| value.value().to_vec())
            .ok_or_else(|| {
                OpenReadError::wrap_io_error(
                    io::Error::new(io::ErrorKind::InvalidData, "fulltext blob is missing"),
                    path.to_path_buf(),
                )
            })
    }

    fn replace(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let key = self.key(path)?;
        let blob_key = Sha256::digest(data);
        let transaction = self.db.begin_write().map_err(io_error)?;
        {
            let mut blobs = transaction.open_table(FULLTEXT_BLOBS).map_err(io_error)?;
            blobs.insert(blob_key.as_slice(), data).map_err(io_error)?;
        }
        {
            let mut files = transaction
                .open_table(FULLTEXT_GENERATION_FILES)
                .map_err(io_error)?;
            files
                .insert(key.as_slice(), blob_key.as_slice())
                .map_err(io_error)?;
        }
        transaction.commit().map_err(io_error)
    }

    pub fn list_namespaces(db: &Database) -> io::Result<Vec<Vec<u8>>> {
        let read = db.begin_read().map_err(io_error)?;
        let active = match read.open_table(FULLTEXT_ACTIVE_GENERATIONS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(io_error(error)),
        };
        let mut namespaces = Vec::new();
        for entry in active.iter().map_err(io_error)? {
            let (namespace, _) = entry.map_err(io_error)?;
            namespaces.push(namespace.value().to_vec());
        }
        Ok(namespaces)
    }

    pub fn list_dropped_namespaces(db: &Database) -> io::Result<Vec<Vec<u8>>> {
        let read = db.begin_read().map_err(io_error)?;
        let dropped = match read.open_table(FULLTEXT_DROPPED_NAMESPACES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(io_error(error)),
        };
        let mut namespaces = Vec::new();
        for entry in dropped.iter().map_err(io_error)? {
            let (namespace, _) = entry.map_err(io_error)?;
            namespaces.push(namespace.value().to_vec());
        }
        namespaces.sort();
        Ok(namespaces)
    }

    /// Remove every inactive manifest in one namespace and then delete blobs
    /// that are no longer referenced by any namespace or generation.
    ///
    /// The caller must guarantee that no reader or stager still owns an inactive
    /// generation. Logical databases invoke this only while reopening storage.
    pub fn prune_inactive_generations(
        transaction: &WriteTransaction,
        namespace: &[u8],
    ) -> io::Result<FulltextGcReport> {
        let namespace = validate_namespace(namespace)?;
        let active_generation = {
            let active = transaction
                .open_table(FULLTEXT_ACTIVE_GENERATIONS)
                .map_err(io_error)?;
            active
                .get(namespace.as_ref())
                .map_err(io_error)?
                .map(|value| value.value())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "fulltext namespace '{}' has no active generation",
                            String::from_utf8_lossy(&namespace)
                        ),
                    )
                })?
        };
        let namespace_prefix = namespace_prefix(&namespace);
        let active_prefix = generation_prefix(&namespace, active_generation);
        let generation_files_removed = {
            let mut files = transaction
                .open_table(FULLTEXT_GENERATION_FILES)
                .map_err(io_error)?;
            let stale_keys = prefixed_keys(&files, &namespace_prefix)?
                .into_iter()
                .filter(|key| !key.starts_with(&active_prefix))
                .collect::<Vec<_>>();
            for key in &stale_keys {
                files.remove(key.as_slice()).map_err(io_error)?;
            }
            stale_keys.len()
        };
        let blobs_removed = remove_unreferenced_blobs(transaction)?;
        Ok(FulltextGcReport {
            generation_files_removed,
            blobs_removed,
        })
    }

    /// Purge a namespace previously tombstoned by [`Self::deactivate`]. This is
    /// called only during logical database reopen, after old readers are gone.
    pub fn prune_dropped_namespace(
        transaction: &WriteTransaction,
        namespace: &[u8],
    ) -> io::Result<FulltextGcReport> {
        let namespace = validate_namespace(namespace)?;
        let prefix = namespace_prefix(&namespace);
        let tombstoned = transaction
            .open_table(FULLTEXT_DROPPED_NAMESPACES)
            .map_err(io_error)?
            .get(namespace.as_ref())
            .map_err(io_error)?
            .is_some();
        if !tombstoned {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "fulltext namespace '{}' is not tombstoned",
                    String::from_utf8_lossy(&namespace)
                ),
            ));
        }
        if transaction
            .open_table(FULLTEXT_ACTIVE_GENERATIONS)
            .map_err(io_error)?
            .get(namespace.as_ref())
            .map_err(io_error)?
            .is_some()
        {
            return Err(io::Error::other("tombstoned fulltext namespace is active"));
        }
        transaction
            .open_table(FULLTEXT_GENERATION_COUNTERS)
            .map_err(io_error)?
            .remove(namespace.as_ref())
            .map_err(io_error)?;
        let generation_files_removed = {
            let mut files = transaction
                .open_table(FULLTEXT_GENERATION_FILES)
                .map_err(io_error)?;
            let keys = prefixed_keys(&files, &prefix)?;
            for key in &keys {
                files.remove(key.as_slice()).map_err(io_error)?;
            }
            keys.len()
        };
        transaction
            .open_table(FULLTEXT_DROPPED_NAMESPACES)
            .map_err(io_error)?
            .remove(namespace.as_ref())
            .map_err(io_error)?;
        let blobs_removed = remove_unreferenced_blobs(transaction)?;
        Ok(FulltextGcReport {
            generation_files_removed,
            blobs_removed,
        })
    }
}

fn remove_unreferenced_blobs(transaction: &WriteTransaction) -> io::Result<usize> {
    let referenced_blobs = {
        let files = transaction
            .open_table(FULLTEXT_GENERATION_FILES)
            .map_err(io_error)?;
        let mut referenced = HashSet::new();
        for entry in files.iter().map_err(io_error)? {
            let (_, blob_key) = entry.map_err(io_error)?;
            referenced.insert(blob_key.value().to_vec());
        }
        referenced
    };
    let mut blobs = transaction.open_table(FULLTEXT_BLOBS).map_err(io_error)?;
    let mut stale_keys = Vec::new();
    for entry in blobs.iter().map_err(io_error)? {
        let (key, _) = entry.map_err(io_error)?;
        if !referenced_blobs.contains(key.value()) {
            stale_keys.push(key.value().to_vec());
        }
    }
    for key in &stale_keys {
        blobs.remove(key.as_slice()).map_err(io_error)?;
    }
    Ok(stale_keys.len())
}

fn validate_namespace(namespace: &[u8]) -> io::Result<Arc<[u8]>> {
    if namespace.is_empty() || namespace.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fulltext namespace must be non-empty and cannot contain NUL",
        ));
    }
    Ok(Arc::from(namespace))
}

fn initialize_tables(transaction: &WriteTransaction) -> io::Result<()> {
    transaction.open_table(FULLTEXT_BLOBS).map_err(io_error)?;
    transaction
        .open_table(FULLTEXT_GENERATION_FILES)
        .map_err(io_error)?;
    transaction
        .open_table(FULLTEXT_ACTIVE_GENERATIONS)
        .map_err(io_error)?;
    transaction
        .open_table(FULLTEXT_GENERATION_COUNTERS)
        .map_err(io_error)?;
    transaction
        .open_table(FULLTEXT_DROPPED_NAMESPACES)
        .map_err(io_error)?;
    Ok(())
}

fn allocate_generation(
    db: &Database,
    namespace: &[u8],
    clone_from: Option<u64>,
) -> io::Result<u64> {
    let transaction = db.begin_write().map_err(io_error)?;
    initialize_tables(&transaction)?;
    let generation = {
        let mut counters = transaction
            .open_table(FULLTEXT_GENERATION_COUNTERS)
            .map_err(io_error)?;
        let current = counters
            .get(namespace)
            .map_err(io_error)?
            .map(|value| value.value())
            .unwrap_or(0);
        let next = current
            .checked_add(1)
            .ok_or_else(|| io::Error::other("fulltext generation counter overflow"))?;
        counters.insert(namespace, next).map_err(io_error)?;
        next
    };

    if let Some(source_generation) = clone_from {
        let source_prefix = generation_prefix(namespace, source_generation);
        let target_prefix = generation_prefix(namespace, generation);
        let entries = {
            let files = transaction
                .open_table(FULLTEXT_GENERATION_FILES)
                .map_err(io_error)?;
            prefixed_entries(&files, &source_prefix)?
        };
        let mut files = transaction
            .open_table(FULLTEXT_GENERATION_FILES)
            .map_err(io_error)?;
        for (source_key, blob_key) in entries {
            let suffix = &source_key[source_prefix.len()..];
            let mut target_key = target_prefix.clone();
            target_key.extend_from_slice(suffix);
            files
                .insert(target_key.as_slice(), blob_key.as_slice())
                .map_err(io_error)?;
        }
    }
    transaction.commit().map_err(io_error)?;
    Ok(generation)
}

fn active_generation(db: &Database, namespace: &[u8]) -> io::Result<Option<u64>> {
    let read = db.begin_read().map_err(io_error)?;
    let active = match read.open_table(FULLTEXT_ACTIVE_GENERATIONS) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    active
        .get(namespace)
        .map_err(io_error)
        .map(|value| value.map(|value| value.value()))
}

fn namespace_prefix(namespace: &[u8]) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(namespace.len() + 1);
    prefix.extend_from_slice(namespace);
    prefix.push(0);
    prefix
}

fn generation_prefix(namespace: &[u8], generation: u64) -> Vec<u8> {
    let mut prefix = namespace_prefix(namespace);
    prefix.extend_from_slice(&generation.to_be_bytes());
    prefix.push(0);
    prefix
}

fn generation_file_key(namespace: &[u8], generation: u64, path: &Path) -> io::Result<Vec<u8>> {
    let path = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Tantivy path is not valid UTF-8: {path:?}"),
        )
    })?;
    let mut key = generation_prefix(namespace, generation);
    key.extend_from_slice(path.as_bytes());
    Ok(key)
}

fn prefixed_keys(table: &redb::Table<&[u8], &[u8]>, prefix: &[u8]) -> io::Result<Vec<Vec<u8>>> {
    Ok(prefixed_entries(table, prefix)?
        .into_iter()
        .map(|(key, _)| key)
        .collect())
}

fn prefixed_entries(
    table: &redb::Table<&[u8], &[u8]>,
    prefix: &[u8],
) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    table
        .range::<&[u8]>(prefix..)
        .map_err(io_error)?
        .map(|entry| {
            let (key, value) = entry.map_err(io_error)?;
            Ok((key.value().to_vec(), value.value().to_vec()))
        })
        .take_while(|entry: &io::Result<(Vec<u8>, Vec<u8>)>| {
            entry
                .as_ref()
                .map(|(key, _)| key.starts_with(prefix))
                .unwrap_or(true)
        })
        .collect()
}

struct RedbWriter {
    directory: RedbDirectory,
    path: PathBuf,
    data: Cursor<Vec<u8>>,
}

impl Write for RedbWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.data.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.directory.replace(&self.path, self.data.get_ref())
    }
}

impl Seek for RedbWriter {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.data.seek(position)
    }
}

impl TerminatingWrite for RedbWriter {
    fn terminate_ref(&mut self, _: AntiCallToken) -> io::Result<()> {
        self.flush()
    }
}

impl Directory for RedbDirectory {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        Ok(Arc::new(FileSlice::from(self.read_bytes(path)?)))
    }

    fn delete(&self, path: &Path) -> Result<(), DeleteError> {
        let key = self.key(path).map_err(|error| DeleteError::IoError {
            io_error: Arc::new(error),
            filepath: path.to_path_buf(),
        })?;
        let transaction = self
            .db
            .begin_write()
            .map_err(|error| DeleteError::IoError {
                io_error: Arc::new(io_error(error)),
                filepath: path.to_path_buf(),
            })?;
        let removed = {
            let mut files = transaction
                .open_table(FULLTEXT_GENERATION_FILES)
                .map_err(|error| DeleteError::IoError {
                    io_error: Arc::new(io_error(error)),
                    filepath: path.to_path_buf(),
                })?;
            files
                .remove(key.as_slice())
                .map_err(|error| DeleteError::IoError {
                    io_error: Arc::new(io_error(error)),
                    filepath: path.to_path_buf(),
                })?
                .is_some()
        };
        if !removed {
            return Err(DeleteError::FileDoesNotExist(path.to_path_buf()));
        }
        transaction.commit().map_err(|error| DeleteError::IoError {
            io_error: Arc::new(io_error(error)),
            filepath: path.to_path_buf(),
        })?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> Result<bool, OpenReadError> {
        match self.read_bytes(path) {
            Ok(_) => Ok(true),
            Err(OpenReadError::FileDoesNotExist(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn open_write(&self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        match self.exists(path) {
            Ok(true) => return Err(OpenWriteError::FileAlreadyExists(path.to_path_buf())),
            Err(error) => {
                return Err(OpenWriteError::wrap_io_error(
                    io_error(error),
                    path.to_path_buf(),
                ));
            }
            Ok(false) => {}
        }
        self.replace(path, &[])
            .map_err(|error| OpenWriteError::wrap_io_error(error, path.to_path_buf()))?;
        Ok(BufWriter::new(Box::new(RedbWriter {
            directory: self.clone(),
            path: path.to_path_buf(),
            data: Cursor::new(Vec::new()),
        })))
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        self.read_bytes(path)
    }

    fn atomic_write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        self.replace(path, data)?;
        if path == Path::new("meta.json") {
            drop(self.watch_callbacks.broadcast());
        }
        Ok(())
    }

    fn sync_directory(&self) -> io::Result<()> {
        Ok(())
    }

    fn acquire_lock(&self, lock: &Lock) -> Result<DirectoryLock, LockError> {
        let mut held = self.locks.held.lock();
        if lock.is_blocking {
            while held.contains(&lock.filepath) {
                self.locks.available.wait(&mut held);
            }
        } else if held.contains(&lock.filepath) {
            return Err(LockError::LockBusy);
        }
        held.insert(lock.filepath.clone());
        drop(held);
        Ok(DirectoryLock::from(Box::new(RedbLockGuard {
            state: self.locks.clone(),
            path: lock.filepath.clone(),
        })))
    }

    fn watch(&self, callback: WatchCallback) -> tantivy::Result<WatchHandle> {
        Ok(self.watch_callbacks.subscribe(callback))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::directory::Directory;

    #[test]
    fn staged_generation_is_invisible_until_activation() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let db = Arc::new(Database::create(temp.path().join("index.redb"))?);
        let first = RedbDirectory::stage_empty(db.clone(), b"default")?;
        first.atomic_write(Path::new("meta.json"), b"first")?;
        assert!(RedbDirectory::open_active(db.clone(), b"default").is_err());
        first.activate_standalone()?;

        let second = RedbDirectory::stage_from_active(db.clone(), b"default")?;
        second.atomic_write(Path::new("meta.json"), b"second")?;
        let active = RedbDirectory::open_active(db.clone(), b"default")?;
        assert_eq!(active.atomic_read(Path::new("meta.json"))?, b"first");
        second.activate_standalone()?;
        let active = RedbDirectory::open_active(db, b"default")?;
        assert_eq!(active.atomic_read(Path::new("meta.json"))?, b"second");
        Ok(())
    }

    #[test]
    fn activation_can_be_aborted_by_external_transaction() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = tempfile::tempdir()?;
        let db = Arc::new(Database::create(temp.path().join("index.redb"))?);
        let first = RedbDirectory::stage_empty(db.clone(), b"default")?;
        first.atomic_write(Path::new("meta.json"), b"first")?;
        first.activate_standalone()?;
        let second = RedbDirectory::stage_from_active(db.clone(), b"default")?;
        second.atomic_write(Path::new("meta.json"), b"second")?;

        let transaction = db.begin_write()?;
        second.staged_generation().activate(&transaction)?;
        transaction.abort()?;

        let active = RedbDirectory::open_active(db, b"default")?;
        assert_eq!(active.atomic_read(Path::new("meta.json"))?, b"first");
        Ok(())
    }

    #[test]
    fn pruning_keeps_active_manifests_and_shared_blobs() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let db = Arc::new(Database::create(temp.path().join("index.redb"))?);
        let first = RedbDirectory::stage_empty(db.clone(), b"default")?;
        first.atomic_write(Path::new("shared"), b"shared")?;
        first.atomic_write(Path::new("old"), b"old")?;
        first.activate_standalone()?;

        let second = RedbDirectory::stage_from_active(db.clone(), b"default")?;
        second.delete(Path::new("old"))?;
        second.atomic_write(Path::new("new"), b"new")?;
        let transaction = db.begin_write()?;
        second.staged_generation().activate(&transaction)?;
        let report = RedbDirectory::prune_inactive_generations(&transaction, b"default")?;
        transaction.commit()?;

        assert_eq!(report.generation_files_removed, 2);
        assert_eq!(report.blobs_removed, 1);
        let active = RedbDirectory::open_active(db, b"default")?;
        assert_eq!(active.atomic_read(Path::new("shared"))?, b"shared");
        assert_eq!(active.atomic_read(Path::new("new"))?, b"new");
        assert!(active.atomic_read(Path::new("old")).is_err());
        assert!(first.atomic_read(Path::new("shared")).is_err());
        Ok(())
    }

    #[test]
    fn isolates_namespaces() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let db = Arc::new(Database::create(temp.path().join("index.redb"))?);
        let left = RedbDirectory::stage_empty(db.clone(), b"left")?;
        let right = RedbDirectory::stage_empty(db, b"right")?;
        left.atomic_write(Path::new("meta.json"), b"left")?;
        assert!(!right.exists(Path::new("meta.json"))?);
        Ok(())
    }

    #[test]
    fn reopens_committed_tantivy_index() -> Result<(), Box<dyn std::error::Error>> {
        use crate::{ChunkDocument, FulltextConfig, FulltextIndex};
        use zeta4j_common::CanonicalEntityRef;

        let temp = tempfile::tempdir()?;
        let db = Arc::new(Database::create(temp.path().join("index.redb"))?);
        let index = FulltextIndex::create_in_redb(
            db.clone(),
            b"search",
            FulltextConfig::default().without_auto_commit(),
        )?;
        index.writer().add_document(&ChunkDocument::new(
            7,
            "redb unified fulltext storage",
            CanonicalEntityRef::GraphNode {
                element_id: "7".to_string(),
            },
        ))?;
        index.commit()?;
        drop(index);

        let reopened = FulltextIndex::open_in_redb(
            db.clone(),
            b"search",
            FulltextConfig::default().without_auto_commit(),
        )?;
        let results = reopened.reader().search_bm25("unified", 10)?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, 7);
        let write_error = reopened.writer().add_document(&ChunkDocument::new(
            8,
            "must not write active generation",
            CanonicalEntityRef::GraphNode {
                element_id: "8".to_string(),
            },
        ));
        assert!(write_error.is_err());
        assert_eq!(
            RedbDirectory::list_namespaces(&db)?,
            vec![b"search".to_vec()]
        );
        drop(reopened);
        let transaction = db.begin_write()?;
        assert!(RedbDirectory::deactivate(&transaction, b"search")?);
        transaction.commit()?;
        assert_eq!(
            RedbDirectory::list_dropped_namespaces(&db)?,
            vec![b"search".to_vec()]
        );
        let transaction = db.begin_write()?;
        let report = RedbDirectory::prune_dropped_namespace(&transaction, b"search")?;
        transaction.commit()?;
        assert!(report.generation_files_removed > 0);
        assert!(RedbDirectory::list_namespaces(&db)?.is_empty());
        assert!(RedbDirectory::list_dropped_namespaces(&db)?.is_empty());
        Ok(())
    }
}
