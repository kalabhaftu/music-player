use rusqlite::{Connection, Result};
use std::path::Path;

type WriteTask = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

/// Single dedicated database writer actor.
/// In SQLite WAL mode, only one thread may write at a time.
/// By routing all mutating operations through this actor, write-lock
/// contention and SQLITE_BUSY deadlocks are eliminated while readers
/// can query simultaneously on independent connections.
#[derive(Clone)]
pub struct LibraryWriter {
    sender: crossbeam_channel::Sender<WriteTask>,
}

impl LibraryWriter {
    pub fn start<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (sender, receiver) = crossbeam_channel::unbounded::<WriteTask>();

        // Open and configure initial connection synchronously to ensure valid schema
        let initial_conn = Connection::open(&db_path)?;
        initial_conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        initial_conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -64000;
             PRAGMA mmap_size = 268435456;
             PRAGMA busy_timeout = 5000;",
        )?;
        super::schema::initialize_schema(&initial_conn)?;
        super::functions::register_query_functions(&initial_conn);

        std::thread::Builder::new()
            .name("library-db-writer".to_string())
            .spawn(move || {
                let mut conn = initial_conn;
                while let Ok(task) = receiver.recv() {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        task(&mut conn);
                    }));

                    // Drain immediately pending write tasks to batch transactions where applicable
                    while let Ok(next) = receiver.try_recv() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            next(&mut conn);
                        }));
                    }
                }
            })
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        Ok(Self { sender })
    }

    /// Executes a write operation synchronously on the writer thread and returns its result.
    pub fn execute<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.sender
            .send(Box::new(move |conn: &mut Connection| {
                let res = f(conn);
                let _ = tx.send(res);
            }))
            .map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;

        rx.recv().map_err(|_| rusqlite::Error::ExecuteReturnedResults)?
    }

    /// Queues a write operation asynchronously on the writer thread without waiting for its completion.
    pub fn execute_async<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Connection) -> Result<()> + Send + 'static,
    {
        self.sender
            .send(Box::new(move |conn: &mut Connection| {
                let _ = f(conn);
            }))
            .map_err(|_| rusqlite::Error::ExecuteReturnedResults)
    }
}
