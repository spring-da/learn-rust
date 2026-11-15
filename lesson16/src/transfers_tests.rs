use super::*;
use crate::api;
use axum::{Router, body::{Body, to_bytes}, http::{Request, header}, response::Response};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{sync::Barrier, task::JoinSet};
use tower::ServiceExt;

async fn fixture(from_points: i64, to_points: i64) -> (tempfile::TempDir, Arc<SqliteUserRepository>) {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("transfers.db")).await.unwrap());
    repo.create("from-user".into(), "Alice".into(), from_points).await.unwrap();
    repo.create("to-user".into(), "Bob".into(), to_points).await.unwrap();
    (directory, repo)
}

async fn assert_state(repo: &SqliteUserRepository, fp: i64, tp: i64, fv: i64, tv: i64, transfers: i64) {
    let from = repo.find_by_id(1).await.unwrap().unwrap();
    let to = repo.find_by_id(2).await.unwrap().unwrap();
    assert_eq!((from.points, to.points, from.version, to.version), (fp, tp, fv, tv));
    let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts), (SELECT COUNT(*) FROM idempotency_records), (SELECT COUNT(*) FROM outbox_events), (SELECT COUNT(*) FROM transfers)"
    ).fetch_one(repo.pool()).await.unwrap();
    assert_eq!(counts, (2, 2, 2, 2, transfers));
}

async fn request(app: &Router, method: &str, path: &str, content_type: Option<&str>, body: String, key: Option<&str>) -> Response {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(content_type) = content_type { request = request.header(header::CONTENT_TYPE, content_type); }
    if let Some(key) = key { request = request.header("Idempotency-Key", key); }
    app.clone().oneshot(request.body(Body::from(body)).unwrap()).await.unwrap()
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}

#[tokio::test]
async fn transfer_http_replay_scope_and_versions() {
    let (_directory, repo) = fixture(100, 20).await;
    let app = api::app(Arc::clone(&repo));
    let payload = json!({"from_user_id":1,"to_user_id":2,"amount":30}).to_string();
    let first = request(&app,"POST","/transfers",Some("application/json"),payload.clone(),Some("from-user")).await;
    assert_eq!(first.status().as_u16(),201);
    assert_eq!(first.headers()["idempotency-replayed"],"false");
    assert_eq!(first.headers()[header::LOCATION],"/transfers/1");
    let original=json_body(first).await;
    assert_eq!(original,json!({"id":1,"from_user_id":1,"to_user_id":2,"amount":30,"from_points":70,"to_points":50,"from_version":1,"to_version":1}));
    let replay=request(&app,"POST","/transfers",Some("application/json"),payload,Some("from-user")).await;
    assert_eq!(replay.status().as_u16(),201);
    assert_eq!(replay.headers()["idempotency-replayed"],"true");
    assert_eq!(json_body(replay).await,original);
    let fetched=request(&app,"GET","/transfers/1",None,String::new(),None).await;
    assert_eq!(fetched.status().as_u16(),200);
    assert_eq!(json_body(fetched).await,original);
    let conflict=request(&app,"POST","/transfers",Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":31}).to_string(),Some("from-user")).await;
    assert_eq!(conflict.status().as_u16(),409);
    let stale=request(&app,"PUT","/users/1/points",Some("application/json"),json!({"points":100,"expected_version":0}).to_string(),None).await;
    assert_eq!(stale.status().as_u16(),409);
    assert_state(&repo,70,50,1,1,1).await;
    repo.close().await;
}

#[tokio::test]
async fn invalid_transfer_requests_do_not_change_accounts() {
    let (_directory,repo)=fixture(100,20).await;
    let app=api::app(Arc::clone(&repo));
    let valid=json!({"from_user_id":1,"to_user_id":2,"amount":30}).to_string();
    let cases=vec![
        (Some("application/json"),valid.clone(),None,400),
        (Some("application/json"),valid.clone(),Some("bad key"),400),
        (None,valid.clone(),Some("k"),415),
        (Some("text/plain"),valid.clone(),Some("k"),415),
        (Some("application/json"),"null".into(),Some("k"),400),
        (Some("application/json"),"[1,2,30]".into(),Some("k"),400),
        (Some("application/json"),"{}".into(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2}).to_string(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":null,"to_user_id":2,"amount":30}).to_string(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":null}).to_string(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":"30"}).to_string(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":1.5}).to_string(),Some("k"),400),
        (Some("application/json"),r#"{"from_user_id":1,"to_user_id":2,"amount":9223372036854775808}"#.into(),Some("k"),400),
        (Some("application/json"),valid.clone()+" {}",Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":30,"extra":1}).to_string(),Some("k"),400),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":1,"amount":30}).to_string(),Some("k"),422),
        (Some("application/json"),json!({"from_user_id":0,"to_user_id":2,"amount":30}).to_string(),Some("k"),422),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":0}).to_string(),Some("k"),422),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":-1}).to_string(),Some("k"),422),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":99,"amount":30}).to_string(),Some("k"),404),
        (Some("application/json"),json!({"from_user_id":1,"to_user_id":2,"amount":1000}).to_string(),Some("k"),409),
        (Some("application/json")," ".repeat(1100)+&valid,Some("k"),413),
    ];
    for (content_type,body,key,expected) in cases {
        let response=request(&app,"POST","/transfers",content_type,body.clone(),key).await;
        assert_eq!(response.status().as_u16(),expected,"body={body}");
        assert!(json_body(response).await["error"].is_string());
    }
    let duplicate=app.clone().oneshot(Request::builder().method("POST").uri("/transfers")
        .header(header::CONTENT_TYPE,"application/json").header("Idempotency-Key","one").header("Idempotency-Key","two")
        .body(Body::from(valid)).unwrap()).await.unwrap();
    assert_eq!(duplicate.status().as_u16(),400);
    for (path,expected) in [("/transfers/nope",400),("/transfers/0",400),("/transfers/99",404),("/transfers",405)] {
        assert_eq!(request(&app,"GET",path,None,String::new(),None).await.status().as_u16(),expected);
    }
    assert_state(&repo,100,20,0,0,0).await;
    repo.close().await;
}

#[tokio::test]
async fn transfer_snapshot_persists_after_other_updates() {
    let (directory,repo)=fixture(100,20).await;
    let first=repo.transfer("persist-transfer".into(),1,2,30).await.unwrap();
    assert!(!first.replayed);
    repo.replace_points(1,80,1).await.unwrap();
    repo.transfer("other-transfer".into(),2,1,5).await.unwrap();
    repo.close().await;
    let reopened=SqliteUserRepository::open(&directory.path().join("transfers.db")).await.unwrap();
    let replay=reopened.transfer("persist-transfer".into(),1,2,30).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.transfer,first.transfer);
    for (from,to,amount) in [(1,2,31),(2,1,30)] {
        assert!(matches!(reopened.transfer("persist-transfer".into(),from,to,amount).await,Err(RepositoryError::TransferKeyMismatch)));
    }
    assert_state(&reopened,85,45,3,2,2).await;
    reopened.close().await;
}

#[tokio::test]
async fn transfer_credit_and_receipt_failures_rollback_both_accounts() {
    let triggers=[
        "CREATE TRIGGER reject_write BEFORE UPDATE ON points_accounts WHEN NEW.user_id = 2 BEGIN SELECT RAISE(ABORT, 'simulated credit failure'); END;",
        "CREATE TRIGGER reject_write BEFORE INSERT ON transfers BEGIN SELECT RAISE(ABORT, 'simulated receipt failure'); END;",
    ];
    for trigger in triggers {
        let (_directory,repo)=fixture(100,20).await;
        sqlx::raw_sql(trigger).execute(repo.pool()).await.unwrap();
        assert!(repo.transfer("retry-key".into(),1,2,30).await.is_err());
        assert_state(&repo,100,20,0,0,0).await;
        sqlx::query("DROP TRIGGER reject_write").execute(repo.pool()).await.unwrap();
        assert!(!repo.transfer("retry-key".into(),1,2,30).await.unwrap().replayed);
        assert_state(&repo,70,50,1,1,1).await;
        repo.close().await;
    }
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn concurrent_same_transfer_key_across_pools() {
    let (directory,first)=fixture(100,20).await;
    let second=Arc::new(SqliteUserRepository::open(&directory.path().join("transfers.db")).await.unwrap());
    let start=Arc::new(Barrier::new(32));
    let mut tasks=JoinSet::new();
    for index in 0..32 {
        let repo=if index%2==0 { Arc::clone(&first) } else { Arc::clone(&second) };
        let start=Arc::clone(&start);
        tasks.spawn(async move { start.wait().await; repo.transfer("same-key".into(),1,2,30).await });
    }
    let (mut created,mut replayed)=(0,0);
    let mut expected=None;
    while let Some(outcome)=tasks.join_next().await {
        let result=outcome.unwrap().unwrap();
        if let Some(receipt)=&expected { assert_eq!(&result.transfer,receipt); } else { expected=Some(result.transfer); }
        if result.replayed { replayed+=1; } else { created+=1; }
    }
    assert_eq!((created,replayed),(1,31));
    assert_state(&first,70,50,1,1,1).await;
    first.close().await; second.close().await;
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn concurrent_distinct_transfer_keys_cannot_overdraw() {
    let (directory,first)=fixture(100,0).await;
    let second=Arc::new(SqliteUserRepository::open(&directory.path().join("transfers.db")).await.unwrap());
    let start=Arc::new(Barrier::new(32));
    let mut tasks=JoinSet::new();
    for index in 0..32 {
        let repo=if index%2==0 { Arc::clone(&first) } else { Arc::clone(&second) };
        let start=Arc::clone(&start);
        tasks.spawn(async move { start.wait().await; repo.transfer(format!("transfer-{index}"),1,2,10).await });
    }
    let (mut accepted,mut rejected)=(0,0);
    while let Some(outcome)=tasks.join_next().await {
        match outcome.unwrap() {
            Ok(_)=>accepted+=1,
            Err(RepositoryError::InsufficientPoints)=>rejected+=1,
            Err(error)=>panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!((accepted,rejected),(10,22));
    assert_state(&first,0,100,10,10,10).await;
    first.close().await; second.close().await;
}

#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn opposite_transfers_conserve_points() {
    let (directory,first)=fixture(100,100).await;
    let second=Arc::new(SqliteUserRepository::open(&directory.path().join("transfers.db")).await.unwrap());
    let start=Arc::new(Barrier::new(32));
    let mut tasks=JoinSet::new();
    for index in 0..32 {
        let (repo,from,to)=if index%2==0 { (Arc::clone(&first),1,2) } else { (Arc::clone(&second),2,1) };
        let start=Arc::clone(&start);
        tasks.spawn(async move { start.wait().await; repo.transfer(format!("opposite-{index}"),from,to,1).await });
    }
    while let Some(outcome)=tasks.join_next().await { outcome.unwrap().unwrap(); }
    assert_state(&first,100,100,32,32,32).await;
    first.close().await; second.close().await;
}

#[tokio::test]
async fn transfer_bounds_and_failed_key_reuse() {
    {
        let (_directory,repo)=fixture(100,20).await;
        for (from,to,amount) in [(0,2,10),(1,1,10),(1,2,0),(1,2,-1)] {
            assert!(matches!(repo.transfer("bad".into(),from,to,amount).await,Err(RepositoryError::InvalidTransfer)));
        }
        assert!(matches!(repo.transfer("missing".into(),1,99,10).await,Err(RepositoryError::UserNotFound)));
        assert!(matches!(repo.transfer("insufficient".into(),1,2,1000).await,Err(RepositoryError::InsufficientPoints)));
        assert_state(&repo,100,20,0,0,0).await;
        assert!(!repo.transfer("insufficient".into(),1,2,10).await.unwrap().replayed);
        assert_state(&repo,90,30,1,1,1).await;
        repo.close().await;
    }
    {
        let (_directory,repo)=fixture(100,i64::MAX).await;
        assert!(matches!(repo.transfer("overflow".into(),1,2,1).await,Err(RepositoryError::PointsOverflow)));
        assert_state(&repo,100,i64::MAX,0,0,0).await;
        repo.close().await;
    }
    {
        let (_directory,repo)=fixture(100,20).await;
        sqlx::query("UPDATE points_accounts SET version = ? WHERE user_id = 1").bind(i64::MAX).execute(repo.pool()).await.unwrap();
        assert!(matches!(repo.transfer("version-limit".into(),1,2,1).await,Err(RepositoryError::VersionExhausted)));
        assert_state(&repo,100,20,i64::MAX,0,0).await;
        repo.close().await;
    }
    {
        let (_directory,repo)=fixture(i64::MAX,0).await;
        repo.transfer("max".into(),1,2,i64::MAX).await.unwrap();
        assert_state(&repo,0,i64::MAX,1,1,1).await;
        repo.close().await;
    }
}
