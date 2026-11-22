use lesson23::{Error, Pool, Status, Ticket};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn wait(ticket: &Ticket) -> lesson23::Snapshot { ticket.wait(Duration::from_secs(3)).unwrap() }

#[derive(Clone)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);
impl Gate {
    fn new() -> Self { Self(Arc::new((Mutex::new(false), Condvar::new()))) }
    fn wait(&self) {
        let (lock, changed) = &*self.0;
        let mut opened = lock.lock().unwrap();
        while !*opened { opened = changed.wait(opened).unwrap(); }
    }
    fn open(&self) { let (lock, changed) = &*self.0; *lock.lock().unwrap() = true; changed.notify_all(); }
}
struct ReleaseOnDrop(Gate);
impl Drop for ReleaseOnDrop { fn drop(&mut self) { self.0.open(); } }

#[test]
fn capacity_bounds_workers_and_rejects_full_queue() {
    let mut pool = Pool::new(2, 1).unwrap();
    let gate = Gate::new();
    let _release = ReleaseOnDrop(gate.clone()); // 在 pool 之前 Drop，断言失败也释放 gate。
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (started, starts) = mpsc::channel();
    let job = || {
        let (gate, active, peak, started) = (gate.clone(), active.clone(), peak.clone(), started.clone());
        move |_: &lesson23::Cancellation, _: usize| {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(count, Ordering::SeqCst);
            started.send(()).unwrap();
            gate.wait();
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    };
    let first = pool.submit(1, job()).unwrap();
    starts.recv_timeout(Duration::from_secs(3)).unwrap();
    let second = pool.submit(1, job()).unwrap();
    starts.recv_timeout(Duration::from_secs(3)).unwrap();
    let third = pool.submit(1, job()).unwrap();
    assert!(matches!(pool.submit(1, job()), Err(Error::QueueFull)));
    gate.open();
    for ticket in [first, second, third] { assert_eq!(wait(&ticket).status, Status::Succeeded); }
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    pool.close().unwrap();
}

#[test]
fn retry_budget_and_empty_errors_are_handled() {
    let pool = Pool::new(1, 4).unwrap();
    let success = pool.submit(3, |_, attempt| if attempt < 3 { Err("retry".into()) } else { Ok(()) }).unwrap();
    let failure = pool.submit(2, |_, _| Err(String::new())).unwrap();
    let success = wait(&success);
    assert_eq!((success.status, success.attempts, success.error.as_str()), (Status::Succeeded, 3, ""));
    let failure = wait(&failure);
    assert_eq!((failure.status, failure.attempts, failure.error.as_str()), (Status::Failed, 2, "task failed"));
}

#[test]
fn queued_cancellation_skips_callback() {
    let pool = Pool::new(1, 1).unwrap();
    let gate = Gate::new();
    let _release = ReleaseOnDrop(gate.clone());
    let (start, started) = mpsc::channel();
    let blocker_gate = gate.clone();
    let blocker = pool.submit(1, move |_, _| { start.send(()).unwrap(); blocker_gate.wait(); Ok(()) }).unwrap();
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let queued = pool.submit(1, move |_, _| { counter.fetch_add(1, Ordering::SeqCst); Ok(()) }).unwrap();
    assert!(queued.cancel());
    assert!(!queued.cancel());
    gate.open();
    wait(&blocker);
    let snapshot = wait(&queued);
    assert_eq!((snapshot.status, snapshot.attempts), (Status::Cancelled, 0));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct CancelOnDrop(Ticket);
impl Drop for CancelOnDrop { fn drop(&mut self) { self.0.cancel(); } }

#[test]
fn running_cancellation_and_terminal_state_are_stable() {
    let pool = Pool::new(1, 1).unwrap();
    let (start, started) = mpsc::channel();
    let ticket = pool.submit(3, move |cancel, _| {
        start.send(()).unwrap();
        while !cancel.is_cancelled() { std::thread::sleep(Duration::from_millis(1)); }
        Ok(())
    }).unwrap();
    let _cancel = CancelOnDrop(ticket.clone());
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(ticket.cancel());
    let mut snapshot = wait(&ticket);
    assert_eq!((snapshot.status, snapshot.attempts), (Status::Cancelled, 1));
    assert!(!ticket.cancel());
    snapshot.status = Status::Succeeded;
    assert_eq!(ticket.state().status, Status::Cancelled);
}

#[test]
fn task_panic_is_terminal_and_worker_continues() {
    let pool = Pool::new(1, 3).unwrap();
    let panicking = pool.submit(3, |_, _| panic!("boom")).unwrap();
    let next = pool.submit(1, |_, _| Ok(())).unwrap();
    let snapshot = wait(&panicking);
    assert_eq!((snapshot.status, snapshot.attempts, snapshot.error.as_str()), (Status::Failed, 1, "task panicked"));
    assert_eq!(wait(&next).status, Status::Succeeded);
    assert!(!next.cancel());
}

#[test]
fn close_drains_rejects_new_work_and_validates_configuration() {
    let mut pool = Pool::new(1, 2).unwrap();
    let ticket = pool.submit(1, |_, _| Ok(())).unwrap();
    pool.close().unwrap();
    pool.close().unwrap();
    assert_eq!(ticket.state().status, Status::Succeeded);
    assert!(matches!(pool.submit(1, |_, _| Ok(())), Err(Error::Closed)));
    assert!(matches!(Pool::new(0, 1), Err(Error::InvalidConfig)));
    assert!(matches!(pool.submit(0, |_, _| Ok(())), Err(Error::InvalidTask)));
}

#[test]
fn wait_timeout_does_not_cancel_task() {
    let pool = Pool::new(1, 1).unwrap();
    let gate = Gate::new();
    let _release = ReleaseOnDrop(gate.clone());
    let callback_gate = gate.clone();
    let ticket = pool.submit(1, move |_, _| { callback_gate.wait(); Ok(()) }).unwrap();
    assert!(matches!(ticket.wait(Duration::ZERO), Err(Error::Timeout)));
    gate.open();
    assert_eq!(wait(&ticket).status, Status::Succeeded);
}
