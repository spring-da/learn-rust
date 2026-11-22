use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum Error { InvalidConfig, InvalidTask, QueueFull, Closed, Timeout, WorkerPanicked, Record, Io(std::io::Error) }
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "workers must be 1..16 and queue capacity 1..1024"),
            Self::InvalidTask => write!(f, "task requires 1..8 maximum attempts"),
            Self::QueueFull => write!(f, "task queue is full"),
            Self::Closed => write!(f, "pool is closed"),
            Self::Timeout => write!(f, "wait timed out"),
            Self::WorkerPanicked => write!(f, "worker thread panicked"),
            Self::Record => write!(f, "invalid terminal result record"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error { fn from(error: std::io::Error) -> Self { Self::Io(error) } }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Queued, Running, Succeeded, Failed, Cancelled }
impl Status {
    pub fn terminal(self) -> bool { matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled) }
    pub fn as_str(self) -> &'static str {
        match self { Self::Queued => "queued", Self::Running => "running", Self::Succeeded => "succeeded", Self::Failed => "failed", Self::Cancelled => "cancelled" }
    }
    pub(crate) fn parse(value: &str) -> Result<Self, Error> {
        match value { "succeeded" => Ok(Self::Succeeded), "failed" => Ok(Self::Failed), "cancelled" => Ok(Self::Cancelled), _ => Err(Error::Record) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot { pub id: u64, pub status: Status, pub attempts: usize, pub error: String }
struct State { snapshot: Snapshot, cancel_requested: bool }
struct Shared { state: Mutex<State>, changed: Condvar }

#[derive(Clone)]
pub struct Ticket { shared: Arc<Shared> }
pub struct Cancellation { shared: Arc<Shared> }
impl Cancellation {
    pub fn is_cancelled(&self) -> bool { self.shared.state.lock().unwrap().cancel_requested }
}

impl Ticket {
    pub fn state(&self) -> Snapshot { self.shared.state.lock().unwrap().snapshot.clone() }
    pub fn cancel(&self) -> bool {
        let mut state = self.shared.state.lock().unwrap();
        if state.snapshot.status.terminal() || state.cancel_requested { return false; }
        state.cancel_requested = true;
        true
    }
    pub fn wait(&self, timeout: Duration) -> Result<Snapshot, Error> {
        let start = Instant::now();
        let mut state = self.shared.state.lock().unwrap();
        loop {
            if state.snapshot.status.terminal() { return Ok(state.snapshot.clone()); }
            let remaining = timeout.checked_sub(start.elapsed()).ok_or(Error::Timeout)?;
            let (next, expired) = self.shared.changed.wait_timeout(state, remaining).unwrap();
            state = next;
            if expired.timed_out() && !state.snapshot.status.terminal() { return Err(Error::Timeout); }
        }
    }
}

type Task = Box<dyn FnMut(&Cancellation, usize) -> Result<(), String> + Send + 'static>;
struct Work { ticket: Ticket, max_attempts: usize, run: Task }
struct PoolState { sender: Option<mpsc::SyncSender<Work>>, next_id: u64 }
pub struct Pool { state: Mutex<PoolState>, workers: Vec<JoinHandle<()>> }

impl Pool {
    pub fn new(workers: usize, capacity: usize) -> Result<Self, Error> {
        if !(1..=16).contains(&workers) || !(1..=1024).contains(&capacity) { return Err(Error::InvalidConfig); }
        let (sender, receiver) = mpsc::sync_channel::<Work>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut handles = Vec::new();
        for _ in 0..workers {
            let receiver = receiver.clone();
            handles.push(thread::spawn(move || loop {
                // 只在接收时持有锁；执行回调前释放，其他 worker 才能继续接收。
                let received = { receiver.lock().unwrap().recv() };
                match received { Ok(work) => execute(work), Err(_) => break }
            }));
        }
        Ok(Self { state: Mutex::new(PoolState { sender: Some(sender), next_id: 0 }), workers: handles })
    }

    pub fn submit(
        &self,
        max_attempts: usize,
        run: impl FnMut(&Cancellation, usize) -> Result<(), String> + Send + 'static,
    ) -> Result<Ticket, Error> {
        if !(1..=8).contains(&max_attempts) { return Err(Error::InvalidTask); }
        let mut state = self.state.lock().unwrap();
        if state.sender.is_none() { return Err(Error::Closed); }
        state.next_id = state.next_id.checked_add(1).ok_or(Error::InvalidTask)?;
        let ticket = Ticket { shared: Arc::new(Shared {
            state: Mutex::new(State { snapshot: Snapshot { id: state.next_id, status: Status::Queued, attempts: 0, error: String::new() }, cancel_requested: false }),
            changed: Condvar::new(),
        }) };
        let work = Work { ticket: ticket.clone(), max_attempts, run: Box::new(run) };
        match state.sender.as_ref().unwrap().try_send(work) {
            Ok(()) => Ok(ticket),
            Err(mpsc::TrySendError::Full(_)) => Err(Error::QueueFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(Error::Closed),
        }
    }

    pub fn close(&mut self) -> Result<(), Error> {
        self.state.lock().unwrap().sender.take();
        let mut panicked = false;
        for worker in self.workers.drain(..) { panicked |= worker.join().is_err(); }
        if panicked { Err(Error::WorkerPanicked) } else { Ok(()) }
    }
}
impl Drop for Pool { fn drop(&mut self) { let _ = self.close(); } }

fn execute(mut work: Work) {
    let shared = &work.ticket.shared;
    let cancellation = Cancellation { shared: shared.clone() };
    for attempt in 1..=work.max_attempts {
        {
            let mut state = shared.state.lock().unwrap();
            if state.cancel_requested {
                state.snapshot.status = Status::Cancelled;
                state.snapshot.error = "cancelled".into();
                shared.changed.notify_all();
                return;
            }
            state.snapshot.status = Status::Running;
            state.snapshot.attempts = attempt;
        }
        let result = catch_unwind(AssertUnwindSafe(|| (work.run)(&cancellation, attempt)));
        let mut state = shared.state.lock().unwrap();
        if state.cancel_requested {
            state.snapshot.status = Status::Cancelled;
            state.snapshot.error = "cancelled".into();
        } else {
            match result {
                Err(_) => { state.snapshot.status = Status::Failed; state.snapshot.error = "task panicked".into(); }
                Ok(Ok(())) => { state.snapshot.status = Status::Succeeded; state.snapshot.error.clear(); }
                Ok(Err(error)) => { state.snapshot.error = if error.is_empty() { "task failed".into() } else { error }; if attempt == work.max_attempts { state.snapshot.status = Status::Failed; } }
            }
        }
        if state.snapshot.status.terminal() { shared.changed.notify_all(); return; }
    }
}
