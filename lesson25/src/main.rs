use lesson24::{Spec, Store};
use lesson25::{Clock, Outcome, Policy, step};
use std::time::{SystemTime, UNIX_EPOCH};

struct Manual(i64);
impl Clock for Manual { fn now_ms(&self) -> i64 { self.0 } }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = format!("data/schedule-{stamp}.db");
    let store = Store::open(&path, 0, 64)?;
    let mut spec = Spec { kind: "flaky".into(), payload: "retry me".into(), max_attempts: 3, fail_until: 2, work_ms: 0, delay_ms: 0 };
    let (flaky, _) = store.submit("retry", &spec, 0)?;
    spec.kind = "uppercase".into(); spec.payload = "do not wait".into(); spec.fail_until = 0; spec.max_attempts = 1;
    store.submit("fast", &spec, 0)?;
    let mut clock = Manual(0);
    for index in 0..4 {
        if index >= 2 { clock.0 = store.get(flaky.id)?.next_run_at; }
        let processed = step(&store, Policy { base_ms: 20, cap_ms: 80 }, &clock, 0, |task| {
            if task.spec.kind == "flaky" && task.attempts <= task.spec.fail_until { Ok(Outcome { failure: "temporary".into(), ..Outcome::default() }) }
            else { Ok(Outcome { result: task.spec.payload.clone(), ..Outcome::default() }) }
        })?;
        print!("time={} processed={processed} tasks=", clock.0);
        for task in store.list()? { print!(" [id={} {} attempt={} due={}]", task.id, task.status, task.attempts, task.next_run_at); }
        println!();
    }
    println!("db={path:?} (manual clock, no real backoff sleeping)");
    Ok(())
}
