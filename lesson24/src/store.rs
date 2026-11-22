use crate::{Error, Spec, Task, validate_key};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

const COLUMNS: &str = "id,request_key,kind,payload,max_attempts,fail_until,work_ms,delay_ms,status,attempts,run_token,next_run_at,cancel_requested,result,last_error";
pub struct Store { connection: Mutex<Connection>, _owner: File, capacity: usize }

fn row_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?, request_key: row.get(1)?,
        spec: Spec { kind: row.get(2)?, payload: row.get(3)?, max_attempts: row.get(4)?, fail_until: row.get(5)?, work_ms: row.get(6)?, delay_ms: row.get(7)? },
        status: row.get(8)?, attempts: row.get(9)?, run_token: row.get(10)?, next_run_at: row.get(11)?, cancel_requested: row.get(12)?, result: row.get(13)?, last_error: row.get(14)?,
    })
}

impl Store {
    pub fn open(path: impl AsRef<Path>, now: i64, capacity: usize) -> Result<Self, Error> {
        if now < 0 || !(1..=1024).contains(&capacity) { return Err(Error::Invalid); }
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { std::fs::create_dir_all(parent)?; }
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".owner");
        let owner = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(lock_path)?;
        owner.try_lock().map_err(|error| Error::Owned(error.to_string()))?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.execute("UPDATE tasks SET
 status=CASE WHEN cancel_requested=1 THEN 'cancelled' WHEN attempts>=max_attempts THEN 'failed' ELSE 'queued' END,
 last_error=CASE WHEN cancel_requested=1 THEN 'cancelled' WHEN attempts>=max_attempts THEN 'interrupted: budget exhausted' ELSE 'interrupted: retry on recovery' END,
 result='',run_token=run_token+1,next_run_at=? WHERE status='running'", [now])?;
        Ok(Self { connection: Mutex::new(connection), _owner: owner, capacity })
    }

    pub fn get(&self, id: i64) -> Result<Task, Error> {
        self.connection.lock().unwrap().query_row(&format!("SELECT {COLUMNS} FROM tasks WHERE id=?"), [id], row_task).optional()?.ok_or(Error::NotFound)
    }

    pub fn list(&self) -> Result<Vec<Task>, Error> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(&format!("SELECT {COLUMNS} FROM tasks ORDER BY id LIMIT 100"))?;
        Ok(statement.query_map([], row_task)?.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn submit(&self, key: &str, spec: &Spec, now: i64) -> Result<(Task, bool), Error> {
        validate_key(key)?; spec.validate()?;
        let due = now.checked_add(spec.delay_ms).filter(|_| now >= 0).ok_or(Error::Invalid)?;
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = transaction.query_row(&format!("SELECT {COLUMNS} FROM tasks WHERE request_key=?"), [key], row_task).optional()?;
        if let Some(task) = existing {
            if task.spec != *spec { return Err(Error::Conflict); }
            transaction.commit()?;
            return Ok((task, true));
        }
        let count: i64 = transaction.query_row("SELECT count(*) FROM tasks WHERE status IN ('queued','running')", [], |row| row.get(0))?;
        if count as usize >= self.capacity { return Err(Error::Full); }
        let task = transaction.query_row(&format!("INSERT INTO tasks(request_key,kind,payload,max_attempts,fail_until,work_ms,delay_ms,status,next_run_at) VALUES(?,?,?,?,?,?,?,'queued',?) RETURNING {COLUMNS}"),
            params![key, spec.kind, spec.payload, spec.max_attempts, spec.fail_until, spec.work_ms, spec.delay_ms, due], row_task)?;
        transaction.commit()?;
        Ok((task, false))
    }

    pub fn claim(&self, now: i64) -> Result<Option<Task>, Error> {
        if now < 0 { return Err(Error::Invalid); }
        Ok(self.connection.lock().unwrap().query_row(&format!("UPDATE tasks SET status='running',attempts=attempts+1,run_token=run_token+1 WHERE id=(SELECT id FROM tasks WHERE status='queued' AND cancel_requested=0 AND next_run_at<=? AND attempts<max_attempts ORDER BY next_run_at,id LIMIT 1) RETURNING {COLUMNS}"), [now], row_task).optional()?)
    }

    pub fn finish(&self, id: i64, token: i64, result: &str, failure: &str, next_run_at: i64) -> Result<Task, Error> {
        if next_run_at < 0 || result.len() > 2048 || failure.len() > 2048 { return Err(Error::Invalid); }
        self.connection.lock().unwrap().query_row(&format!("UPDATE tasks SET
 status=CASE WHEN cancel_requested=1 THEN 'cancelled' WHEN ?='' THEN 'succeeded' WHEN attempts>=max_attempts THEN 'failed' ELSE 'queued' END,
 result=CASE WHEN cancel_requested=0 AND ?='' THEN ? ELSE '' END,
 last_error=CASE WHEN cancel_requested=1 THEN 'cancelled' ELSE ? END,
 next_run_at=? WHERE id=? AND status='running' AND run_token=? RETURNING {COLUMNS}"),
            params![failure, failure, result, failure, next_run_at, id, token], row_task).optional()?.ok_or(Error::Stale)
    }

    pub fn cancel(&self, id: i64) -> Result<(Task, bool), Error> {
        let task = self.connection.lock().unwrap().query_row(&format!("UPDATE tasks SET cancel_requested=1,
 status=CASE WHEN status='queued' THEN 'cancelled' ELSE status END,
 last_error=CASE WHEN status='queued' THEN 'cancelled' ELSE last_error END
 WHERE id=? AND status IN ('queued','running') AND cancel_requested=0 RETURNING {COLUMNS}"), [id], row_task).optional()?;
        match task { Some(task) => Ok((task, true)), None => Ok((self.get(id)?, false)) }
    }
}
