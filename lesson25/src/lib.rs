use lesson24::{Error, Store, Task};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH, Instant};

pub trait Clock { fn now_ms(&self) -> i64; }
pub struct RealClock;
impl Clock for RealClock {
    fn now_ms(&self) -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as i64 }
}

#[derive(Clone, Copy)]
pub struct Policy { pub base_ms: i64, pub cap_ms: i64 }
impl Policy {
    pub fn validate(self) -> Result<(), Error> {
        if self.base_ms < 1 || self.cap_ms < self.base_ms || self.cap_ms > 60000 { return Err(Error::Invalid); }
        Ok(())
    }
    pub fn delay(self, attempt: i64, entropy: u64) -> Result<i64, Error> {
        self.validate()?;
        if !(1..=8).contains(&attempt) { return Err(Error::Invalid); }
        let mut ceiling = self.base_ms;
        for _ in 1..attempt { ceiling = (ceiling * 2).min(self.cap_ms); }
        let low = (ceiling / 2).max(1);
        Ok(low + (entropy % (ceiling - low + 1) as u64) as i64)
    }
}

#[derive(Default)]
pub struct Outcome { pub result: String, pub failure: String, pub interrupted: bool }

pub fn step(
    store: &Store, policy: Policy, clock: &impl Clock, entropy: u64,
    execute: impl FnOnce(&Task) -> Result<Outcome, Error>,
) -> Result<bool, Error> {
    policy.validate()?;
    let Some(task) = store.claim(clock.now_ms())? else { return Ok(false); };
    let outcome = execute(&task)?;
    if outcome.interrupted { return Ok(true); }
    let mut due = clock.now_ms();
    if !outcome.failure.is_empty() && task.attempts < task.spec.max_attempts {
        due = due.checked_add(policy.delay(task.attempts, entropy)?).ok_or(Error::Invalid)?;
    }
    store.finish(task.id, task.run_token, &outcome.result, &outcome.failure, due)?;
    Ok(true)
}

pub fn builtin(stop: &AtomicBool, store: &Store, task: &Task) -> Result<Outcome, Error> {
    let start = Instant::now();
    let work = Duration::from_millis(task.spec.work_ms as u64);
    loop {
        if stop.load(Ordering::Relaxed) { return Ok(Outcome { interrupted: true, ..Outcome::default() }); }
        if store.get(task.id)?.cancel_requested { return Ok(Outcome { failure: "cancelled".into(), ..Outcome::default() }); }
        let Some(remaining) = work.checked_sub(start.elapsed()).filter(|time| !time.is_zero()) else { break; };
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    if task.spec.kind == "flaky" && task.attempts <= task.spec.fail_until {
        return Ok(Outcome { failure: "simulated transient failure".into(), ..Outcome::default() });
    }
    Ok(Outcome { result: task.spec.payload.to_ascii_uppercase(), ..Outcome::default() })
}

pub struct Runner { stop: Arc<AtomicBool>, workers: Vec<JoinHandle<()>>, failure: Arc<Mutex<Option<String>>> }
#[derive(Clone)]
pub struct Control { stop: Arc<AtomicBool>, failure: Arc<Mutex<Option<String>>> }
impl Control {
    pub fn request_stop(&self) { self.stop.store(true, Ordering::Relaxed); }
    pub fn error(&self) -> Option<String> { self.failure.lock().unwrap().clone() }
}
impl Runner {
    pub fn control(&self) -> Control { Control { stop: self.stop.clone(), failure: self.failure.clone() } }
    pub fn start(store: Arc<Store>, policy: Policy, workers: usize) -> Result<Self, Error> {
        policy.validate()?;
        if !(1..=4).contains(&workers) { return Err(Error::Invalid); }
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let mut handles = Vec::new();
        for worker in 0..workers {
            let (store, stop, failure) = (store.clone(), stop.clone(), failure.clone());
            handles.push(thread::spawn(move || {
                let mut seed = RealClock.now_ms() as u64 + worker as u64;
                while !stop.load(Ordering::Relaxed) {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    match step(&store, policy, &RealClock, seed, |task| builtin(&stop, &store, task)) {
                        Ok(true) => {},
                        Ok(false) => thread::sleep(Duration::from_millis(10)),
                        Err(error) => {
                            *failure.lock().unwrap() = Some(error.to_string());
                            stop.store(true, Ordering::Relaxed); break;
                        }
                    }
                }
            }));
        }
        Ok(Self { stop, workers: handles, failure })
    }
    pub fn error(&self) -> Option<String> { self.failure.lock().unwrap().clone() }
    pub fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Relaxed);
        for worker in self.workers.drain(..) { if worker.join().is_err() { *self.failure.lock().unwrap() = Some("worker panicked".into()); } }
        match self.error() { Some(error) => Err(error), None => Ok(()) }
    }
}
impl Drop for Runner { fn drop(&mut self) { let _ = self.stop(); } }
