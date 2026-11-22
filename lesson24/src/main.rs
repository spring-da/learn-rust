use lesson24::{Spec, Store};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    let args: Vec<_> = std::env::args().skip(1).collect();
    let list = args.iter().any(|arg| arg == "--list");
    let path = args.windows(2).find(|pair| pair[0] == "--db").map(|pair| pair[1].clone()).unwrap_or_else(|| format!("data/recovery-{now}.db"));
    let store = Store::open(&path, now, 64)?;
    if list { for task in store.list()? { println!("{task:?}"); } return Ok(()); }
    let spec = Spec { kind: "uppercase".into(), payload: "hello recovery".into(), max_attempts: 3, fail_until: 0, work_ms: 0, delay_ms: 0 };
    store.submit("recovery-example", &spec, now)?;
    let old = store.claim(now)?.ok_or("no task to claim")?;
    println!("before interruption: id={} status={} attempts={} token={}", old.id, old.status, old.attempts, old.run_token);
    drop(store);
    let store = Store::open(&path, now + 1, 64)?;
    let recovered = store.get(old.id)?;
    println!("after reopen: status={} attempts={} token={}", recovered.status, recovered.attempts, recovered.run_token);
    println!("old completion rejected: {:?}", store.finish(old.id, old.run_token, "OLD RESULT", "", now + 1));
    let current = store.claim(now + 1)?.ok_or("no recovered task")?;
    let finished = store.finish(current.id, current.run_token, "HELLO RECOVERY", "", now + 1)?;
    println!("db={path:?} status={} attempts={} result={:?}", finished.status, finished.attempts, finished.result);
    Ok(())
}
