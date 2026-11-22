use lesson24::{Spec, Store};
use lesson25::{Clock, Outcome, Policy, RealClock, Runner, builtin, step};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration,Instant};
struct Manual(i64);impl Clock for Manual{fn now_ms(&self)->i64{self.0}}
fn spec()->Spec{Spec{kind:"flaky".into(),payload:"hello".into(),max_attempts:3,fail_until:2,work_ms:0,delay_ms:0}}
fn execute(task:&lesson24::Task)->Result<Outcome,lesson24::Error>{if task.spec.kind=="flaky"&&task.attempts<=task.spec.fail_until{Ok(Outcome{failure:"temporary".into(),..Outcome::default()})}else{Ok(Outcome{result:"OK".into(),..Outcome::default()})}}

#[test]
fn capped_backoff_and_jitter_bounds(){
    let policy=Policy{base_ms:20,cap_ms:70};
    for (index,ceiling) in [20,40,70,70,70,70,70,70].into_iter().enumerate(){for entropy in [0,1,999,u64::MAX]{let delay=policy.delay(index as i64+1,entropy).unwrap();assert!((ceiling/2..=ceiling).contains(&delay));}}
    assert!(Policy{base_ms:0,cap_ms:1}.delay(1,0).is_err());
}

#[test]
fn delayed_retry_releases_worker_for_ready_task(){
    let directory=tempfile::tempdir().unwrap();let store=Store::open(directory.path().join("queue.db"),0,8).unwrap();
    let (flaky,_)=store.submit("retry",&spec(),0).unwrap();let mut fast=spec();fast.kind="uppercase".into();fast.fail_until=0;store.submit("fast",&fast,0).unwrap();
    let policy=Policy{base_ms:20,cap_ms:80};let mut clock=Manual(0);
    step(&store,policy,&clock,0,execute).unwrap();step(&store,policy,&clock,0,execute).unwrap();
    assert_eq!(store.get(2).unwrap().status,"succeeded");assert_eq!(store.get(flaky.id).unwrap().next_run_at,10);assert!(!step(&store,policy,&clock,0,execute).unwrap());
    clock.0=10;step(&store,policy,&clock,0,execute).unwrap();clock.0=30;step(&store,policy,&clock,0,execute).unwrap();
    let task=store.get(flaky.id).unwrap();assert_eq!((task.status.as_str(),task.attempts),("succeeded",3));
}

#[test]
fn retry_deadline_survives_reopen_and_cancel(){
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");let store=Store::open(&path,0,8).unwrap();
    let(task,_)=store.submit("retry",&spec(),0).unwrap();step(&store,Policy{base_ms:20,cap_ms:80},&Manual(0),0,execute).unwrap();drop(store);
    let store=Store::open(&path,1,8).unwrap();assert_eq!(store.get(task.id).unwrap().next_run_at,10);store.cancel(task.id).unwrap();assert!(store.claim(100).unwrap().is_none());
}

#[test]
fn interrupted_attempt_is_recovered(){
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");let store=Store::open(&path,0,8).unwrap();store.submit("interrupt",&spec(),0).unwrap();
    step(&store,Policy{base_ms:20,cap_ms:80},&Manual(0),0,|_|Ok(Outcome{interrupted:true,..Outcome::default()})).unwrap();assert_eq!(store.get(1).unwrap().status,"running");drop(store);
    let store=Store::open(&path,1,8).unwrap();let task=store.get(1).unwrap();assert_eq!((task.status.as_str(),task.attempts),("queued",1));
}

#[test]
fn builtin_ascii_and_running_cancellation(){
    let directory=tempfile::tempdir().unwrap();let store=Store::open(directory.path().join("queue.db"),0,8).unwrap();let mut text=spec();text.kind="uppercase".into();text.fail_until=0;text.payload="hello 中文 ß".into();store.submit("text",&text,0).unwrap();
    let stop=AtomicBool::new(false);step(&store,Policy{base_ms:20,cap_ms:80},&Manual(0),0,|task|builtin(&stop,&store,task)).unwrap();assert_eq!(store.get(1).unwrap().result,"HELLO 中文 ß");
    store.submit("cancel",&text,0).unwrap();let task=store.claim(0).unwrap().unwrap();store.cancel(task.id).unwrap();assert_eq!(builtin(&stop,&store,&task).unwrap().failure,"cancelled");
}

#[test]
fn stopping_runner_leaves_recoverable_work(){
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");let store=Arc::new(Store::open(&path,RealClock.now_ms(),8).unwrap());let mut slow=spec();slow.work_ms=2000;slow.fail_until=0;let(task,_)=store.submit("slow",&slow,RealClock.now_ms()).unwrap();
    let mut runner=Runner::start(store.clone(),Policy{base_ms:20,cap_ms:80},1).unwrap();let start=Instant::now();
    while store.get(task.id).unwrap().status!="running"{assert!(start.elapsed()<Duration::from_secs(3));std::thread::sleep(Duration::from_millis(1));}
    runner.stop().unwrap();assert_eq!(store.get(task.id).unwrap().status,"running");drop(runner);drop(store);
    let store=Store::open(&path,RealClock.now_ms(),8).unwrap();let task=store.get(task.id).unwrap();assert_eq!((task.status.as_str(),task.attempts),("queued",1));
}
