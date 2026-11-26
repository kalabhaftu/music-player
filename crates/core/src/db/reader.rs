use rusqlite::{Connection, OpenFlags, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A pool of independent read-only SQLite connections in WAL mode.
/// Allows multiple worker threads and the UI thread to query the library
/// simultaneously without waiting on or blocking the database writer.
pub struct ReaderPool {
    db_path: PathBuf,
    sender: crossbeam_channel::Sender<Connection>,
    receiver: crossbeam_channel::Receiver<Connection>,
    max_readers: usize,
    created_count: Arc<AtomicUsize>,
}

/// A borrowed connection from the pool. Automatically returned on drop.
pub struct PooledReader {
    conn: Option<Connection>,
    sender: crossbeam_channel::Sender<Connection>,
}

impl std::ops::Deref for PooledReader {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().unwrap()
    }
}

impl Drop for PooledReader {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = self.sender.send(conn);
        }
    }
}

impl ReaderPool {
    pub fn new<P: AsRef<Path>>(db_path: P, max_readers: usize) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let max = max_readers.max(2);
        let (sender, receiver) = crossbeam_channel::bounded(max);

        // Pre-warm 1 reader connection
        let conn = Self::open_reader(&db_path)?;
        let _ = sender.send(conn);
        let created_count = Arc::new(AtomicUsize::new(1));

        Ok(Self {
            db_path,
            sender,
            receiver,
            max_readers: max,
            created_count,
        })
    }

    fn open_reader(path: &Path) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -64000;
             PRAGMA mmap_size = 268435456;
             PRAGMA query_only = ON;",
        )?;
        super::functions::register_query_functions(&conn);
        Ok(conn)
    }

    pub fn acquire(&self) -> Result<PooledReader> {
        if let Ok(conn) = self.receiver.try_recv() {
            return Ok(PooledReader {
                conn: Some(conn),
                sender: self.sender.clone(),
            });
        }

        let count = self.created_count.load(Ordering::SeqCst);
        if count < self.max_readers {
            if self
                .created_count
                .compare_exchange(count, count + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                match Self::open_reader(&self.db_path) {
                    Ok(conn) => {
                        return Ok(PooledReader {
                            conn: Some(conn),
                            sender: self.sender.clone(),
                        });
                    }
                    Err(e) => {
                        self.created_count.fetch_sub(1, Ordering::SeqCst);
                        return Err(e);
                    }
                }
            }
        }

        let conn = self
            .receiver
            .recv()
            .map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;
        Ok(PooledReader {
            conn: Some(conn),
            sender: self.sender.clone(),
        })
    }
}

impl Clone for ReaderPool {
    fn clone(&self) -> Self {
        Self {
            db_path: self.db_path.clone(),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            max_readers: self.max_readers,
            created_count: Arc::clone(&self.created_count),
        }
    }
}
