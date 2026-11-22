use lesson24::{Error, Spec, Store};
use std::sync::Arc;

fn spec() -> Spec { Spec { kind:"uppercase".into(),payload:"hello".into(),max_attempts:3,fail_until:0,work_ms:0,delay_ms:0 } }

#[test]
fn recovery_consumes_attempt_and_fences_old_completion() {
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");
    let store=Store::open(&path,100,4).unwrap();
    let (created,_)=store.submit("same",&spec(),100).unwrap();let old=store.claim(100).unwrap().unwrap();drop(store);
    let store=Store::open(&path,101,4).unwrap();let recovered=store.get(created.id).unwrap();
    assert_eq!((recovered.status.as_str(),recovered.attempts),("queued",1));assert!(recovered.run_token>old.run_token);
    assert!(matches!(store.finish(old.id,old.run_token,"old","",101),Err(Error::Stale)));
    let current=store.claim(101).unwrap().unwrap();assert_eq!(current.attempts,2);
    assert!(matches!(store.finish(old.id,old.run_token,"old","",101),Err(Error::Stale)));
    store.finish(current.id,current.run_token,"HELLO","",101).unwrap();
    let (replayed,yes)=store.submit("same",&spec(),999).unwrap();assert!(yes);assert_eq!((replayed.id,replayed.status.as_str()),(created.id,"succeeded"));
}

#[test]
fn recovery_finalizes_cancellation_and_exhausted_budget() {
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");let store=Store::open(&path,0,4).unwrap();
    let mut one=spec();one.max_attempts=1;
    store.submit("exhausted",&one,0).unwrap();store.submit("cancel",&spec(),0).unwrap();
    let first=store.claim(0).unwrap().unwrap();let second=store.claim(0).unwrap().unwrap();store.cancel(second.id).unwrap();drop(store);
    let store=Store::open(&path,1,4).unwrap();assert_eq!(store.get(first.id).unwrap().status,"failed");assert_eq!(store.get(second.id).unwrap().status,"cancelled");
}

#[test]
fn owner_lock_prevents_live_recovery() {
    let directory=tempfile::tempdir().unwrap();let path=directory.path().join("queue.db");let store=Store::open(&path,0,4).unwrap();
    assert!(matches!(Store::open(&path,1,4),Err(Error::Owned(_))));drop(store);assert!(Store::open(&path,2,4).is_ok());
}

#[test]
fn idempotency_capacity_and_cancelled_queued_work() {
    let directory=tempfile::tempdir().unwrap();let store=Store::open(directory.path().join("queue.db"),0,1).unwrap();
    let (task,_)=store.submit("key",&spec(),0).unwrap();assert!(store.submit("key",&spec(),0).unwrap().1);
    let mut changed=spec();changed.payload="different".into();assert!(matches!(store.submit("key",&changed,0),Err(Error::Conflict)));
    assert!(matches!(store.submit("new",&spec(),0),Err(Error::Full)));
    let (cancelled,yes)=store.cancel(task.id).unwrap();assert!(yes);assert_eq!(cancelled.status,"cancelled");assert!(store.claim(0).unwrap().is_none());
    assert!(store.submit("new",&spec(),0).is_ok());
}

#[test]
fn concurrent_claims_and_conditional_finish() {
    let directory=tempfile::tempdir().unwrap();let store=Arc::new(Store::open(directory.path().join("queue.db"),0,64).unwrap());
    for index in 0..20 { store.submit(&format!("key-{index}"),&spec(),0).unwrap(); }
    let handles:Vec<_>=(0..32).map(|_|{let store=store.clone();std::thread::spawn(move||store.claim(0).unwrap().map(|task|task.id))}).collect();
    let ids:Vec<_>=handles.into_iter().filter_map(|handle|handle.join().unwrap()).collect();
    assert_eq!(ids.len(),20);assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(),20);
    let current=store.get(1).unwrap();store.cancel(1).unwrap();assert_eq!(store.finish(1,current.run_token,"success","",0).unwrap().status,"cancelled");
    assert!(matches!(store.finish(1,current.run_token,"late","",0),Err(Error::Stale)));
}

#[test]
fn due_times_validation_and_missing_task() {
    let directory=tempfile::tempdir().unwrap();let store=Store::open(directory.path().join("queue.db"),0,4).unwrap();
    let mut delayed=spec();delayed.delay_ms=20;store.submit("delayed",&delayed,100).unwrap();
    assert!(store.claim(119).unwrap().is_none());assert!(store.claim(120).unwrap().is_some());assert!(matches!(store.get(999),Err(Error::NotFound)));
    for key in ["","bad key","中文"] {assert!(matches!(store.submit(key,&spec(),0),Err(Error::Invalid)));}
    let mut bad=spec();bad.kind="unknown".into();assert!(matches!(store.submit("bad",&bad,0),Err(Error::Invalid)));
}
