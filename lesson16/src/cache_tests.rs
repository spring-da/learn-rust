use super::*;
use crate::{api,repository::{SqliteUserRepository,User},transfers::TransferReceipt};
use axum::{Router,body::{Body,to_bytes},http::Request,response::Response};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

#[test]
fn generic_types_and_value_ownership() {
    let now=Instant::now();
    let users=TtlCache::<String,User>::new(2,Duration::from_secs(1)).unwrap();
    users.put_at("alice".into(),User{id:1,name:"Alice".into(),points:100,version:0},now).unwrap();
    let mut value=users.get_at(&"alice".into(),now).unwrap();
    value.name.push_str(" changed");
    value.points=999;
    let original=users.get_at(&"alice".into(),now).unwrap();
    assert_eq!((original.name.as_str(),original.points),("Alice",100));
    let labels=TtlCache::<i64,String>::new(2,Duration::from_secs(1)).unwrap();
    labels.put_at(7,"seven".into(),now).unwrap();
    assert_eq!(labels.get_at(&7,now),Some("seven".into()));
}

#[test]
fn ttl_boundary_and_overwrite_does_not_slide_on_read() {
    let base=Instant::now();
    let cache=TtlCache::<String,i32>::new(2,Duration::from_secs(10)).unwrap();
    cache.put_at("a".into(),1,base).unwrap();
    assert_eq!(cache.get_at(&"a".into(),base+Duration::from_secs(9)),Some(1));
    assert_eq!(cache.get_at(&"a".into(),base+Duration::from_secs(10)),None);
    cache.put_at("a".into(),2,base+Duration::from_secs(11)).unwrap();
    cache.put_at("a".into(),3,base+Duration::from_secs(16)).unwrap();
    assert_eq!(cache.get_at(&"a".into(),base+Duration::from_secs(25)),Some(3));
    assert_eq!(cache.get_at(&"a".into(),base+Duration::from_secs(26)),None);
}

#[test]
fn fifo_and_expired_entries_before_eviction() {
    let base=Instant::now();
    let cache=TtlCache::<String,i32>::new(2,Duration::from_secs(10)).unwrap();
    cache.put_at("a".into(),1,base).unwrap();
    cache.put_at("b".into(),2,base+Duration::from_secs(1)).unwrap();
    cache.get_at(&"a".into(),base+Duration::from_secs(2));
    cache.put_at("c".into(),3,base+Duration::from_secs(3)).unwrap();
    assert_eq!(cache.get_at(&"a".into(),base+Duration::from_secs(3)),None);
    assert_eq!(cache.get_at(&"b".into(),base+Duration::from_secs(3)),Some(2));
    cache.put_at("b".into(),20,base+Duration::from_secs(4)).unwrap();
    let stats=cache.stats_at(base+Duration::from_secs(4));
    assert_eq!((stats.entries,stats.evictions),(2,1));
    let fresh=TtlCache::<String,i32>::new(2,Duration::from_secs(10)).unwrap();
    fresh.put_at("expired".into(),1,base).unwrap();
    fresh.put_at("live".into(),2,base+Duration::from_secs(5)).unwrap();
    fresh.put_at("new".into(),3,base+Duration::from_secs(11)).unwrap();
    let stats=fresh.stats_at(base+Duration::from_secs(11));
    assert_eq!((stats.entries,stats.expirations,stats.evictions),(2,1,0));
}

#[test]
fn delete_and_replacement_metadata_stay_bounded() {
    let base=Instant::now();
    let cache=TtlCache::<i32,i32>::new(2,Duration::from_secs(3600)).unwrap();
    for index in 0..2000 {cache.put_at(index%3,index,base).unwrap();}
    {
        let state=cache.state.lock().unwrap();
        assert!(state.items.len()<=2);
        assert_eq!(state.items.len(),state.order.len());
    }
    for key in 0..3 {cache.delete(&key);cache.delete(&key);}
    assert_eq!(cache.stats_at(base).entries,0);
}

#[test]
fn invalid_configuration_and_clock_range_are_rejected() {
    assert!(TtlCache::<i64,String>::new(0,Duration::from_secs(1)).is_err());
    assert!(TtlCache::<i64,String>::new(1,Duration::ZERO).is_err());
    let cache=TtlCache::<String,i32>::new(1,Duration::MAX).unwrap();
    assert!(cache.put_at("huge".into(),1,Instant::now()).is_err());
    assert_eq!(cache.stats_at(Instant::now()).entries,0);
}

#[test]
fn concurrent_access_stays_bounded() {
    let base=Instant::now();
    let cache=Arc::new(TtlCache::<usize,usize>::new(8,Duration::from_secs(3600)).unwrap());
    let mut workers=Vec::new();
    for worker in 0..16 {
        let cache=Arc::clone(&cache);
        workers.push(std::thread::spawn(move || {
            for index in 0..128 {
                let key=(worker+index)%16;
                cache.put_at(key,index,base).unwrap();
                cache.get_at(&key,base);
                if index%3==0 {cache.delete(&key);}
                assert!(cache.stats_at(base).entries<=8);
            }
        }));
    }
    for worker in workers {worker.join().unwrap();}
    let state=cache.state.lock().unwrap();
    assert_eq!(state.items.len(),state.order.len());
    assert!(state.items.len()<=8);
}

async fn fixture()->(tempfile::TempDir,Arc<SqliteUserRepository>,Arc<TtlCache<i64,TransferReceipt>>,Router) {
    let directory=tempfile::tempdir().unwrap();
    let repo=Arc::new(SqliteUserRepository::open(&directory.path().join("cache.db")).await.unwrap());
    repo.create("a".into(),"Alice".into(),100).await.unwrap();
    repo.create("b".into(),"Bob".into(),20).await.unwrap();
    let cache=Arc::new(TtlCache::new(2,Duration::from_secs(1)).unwrap());
    let app=api::app_with_cache(Arc::clone(&repo),Arc::clone(&cache));
    (directory,repo,cache,app)
}

async fn request(app:&Router,method:&str,path:&str)->Response {
    app.clone().oneshot(Request::builder().method(method).uri(path).body(Body::empty()).unwrap()).await.unwrap()
}

async fn body(response:Response)->Value {
    serde_json::from_slice(&to_bytes(response.into_body(),8192).await.unwrap()).unwrap()
}

#[tokio::test]
async fn receipt_cache_http_hit_expiry_delete_and_snapshot() {
    let (_directory,repo,cache,app)=fixture().await;
    let first=repo.transfer("first".into(),1,2,30).await.unwrap();
    let path=format!("/transfers/{}",first.transfer.id);
    let miss=request(&app,"GET",&path).await;
    assert_eq!(miss.status().as_u16(),200);
    assert_eq!(miss.headers()["x-receipt-cache"],"MISS");
    let snapshot=body(miss).await;
    let hit=request(&app,"GET",&path).await;
    assert_eq!(hit.headers()["x-receipt-cache"],"HIT");
    assert_eq!(body(hit).await,snapshot);
    repo.transfer("second".into(),1,2,10).await.unwrap();
    let old=request(&app,"GET",&path).await;
    assert_eq!(old.headers()["x-receipt-cache"],"HIT");
    assert_eq!(body(old).await,snapshot);
    let current=request(&app,"GET","/users/1").await;
    assert!(!current.headers().contains_key("x-receipt-cache"));
    let current=body(current).await;
    assert_eq!((current["points"].as_i64(),current["version"].as_i64()),(Some(60),Some(2)));
    assert_eq!(request(&app,"DELETE",&format!("/cache/receipts/{}",first.transfer.id)).await.status().as_u16(),204);
    assert_eq!(request(&app,"GET",&path).await.headers()["x-receipt-cache"],"MISS");
    cache.put_at(first.transfer.id,first.transfer,Instant::now()-Duration::from_secs(2)).unwrap();
    assert_eq!(request(&app,"GET",&path).await.headers()["x-receipt-cache"],"MISS");
    let stats=body(request(&app,"GET","/cache/stats").await).await;
    assert_eq!(stats["database_loads"],3);
    assert_eq!(stats["hits"],2);
    assert_eq!(stats["expirations"],1);
    repo.close().await;
}

#[tokio::test]
async fn receipt_cache_does_not_cache_not_found() {
    let (_directory,repo,_cache,app)=fixture().await;
    for _ in 0..2 {assert_eq!(request(&app,"GET","/transfers/1").await.status().as_u16(),404);}
    repo.transfer("created-after-miss".into(),1,2,30).await.unwrap();
    let response=request(&app,"GET","/transfers/1").await;
    assert_eq!(response.status().as_u16(),200);
    assert_eq!(response.headers()["x-receipt-cache"],"MISS");
    let stats=body(request(&app,"GET","/cache/stats").await).await;
    assert_eq!(stats["database_loads"],3);
    assert_eq!(stats["entries"],1);
    repo.close().await;
}

#[tokio::test]
async fn receipt_cache_mutex_is_not_held_while_database_waits() {
    let (_directory,repo,_cache,app)=fixture().await;
    repo.transfer("one".into(),1,2,30).await.unwrap();
    let mut held=Vec::new();
    for _ in 0..4 {held.push(repo.pool().acquire().await.unwrap());}
    let lookup_app=app.clone();
    let lookup=tokio::spawn(async move {request(&lookup_app,"GET","/transfers/1").await});
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let stats=tokio::time::timeout(Duration::from_secs(1),async {body(request(&app,"GET","/cache/stats").await).await}).await.expect("cache mutex blocked stats while database pool was busy");
            if stats["database_loads"]==1 {break;}
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }).await.unwrap();
    drop(held);
    let response=tokio::time::timeout(Duration::from_secs(3),lookup).await.unwrap().unwrap();
    assert_eq!(response.status().as_u16(),200);
    repo.close().await;
}
