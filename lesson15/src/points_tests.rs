use super::*;
use crate::api;
use axum::{Router, body::{Body, to_bytes}, http::{Request, header}};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{sync::Barrier, task::JoinSet};
use tower::ServiceExt;

async fn request(app: &Router, method: &str, path: &str, content_type: Option<&str>, body: String) -> (u16, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(content_type) = content_type { request = request.header(header::CONTENT_TYPE, content_type); }
    if method == "POST" && path == "/users" { request = request.header("Idempotency-Key", "test-key"); }
    let response = app.clone().oneshot(request.body(Body::from(body)).unwrap()).await.unwrap();
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn register(repo: &SqliteUserRepository) {
    repo.create("outbox-key".into(), "Alice".into(), 100).await.unwrap();
}

async fn assert_counts(repo: &SqliteUserRepository) {
    let actual: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts), (SELECT COUNT(*) FROM idempotency_records), (SELECT COUNT(*) FROM outbox_events)"
    ).fetch_one(repo.pool()).await.unwrap();
    assert_eq!(actual, (1, 1, 1, 1));
}

#[tokio::test]
async fn points_http_conflict_recalculation_and_registration_replay() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("http.db")).await.unwrap());
    let app = api::app(Arc::clone(&repo));
    let payload = json!({"name":"Alice","initial_points":100}).to_string();
    let (status, original) = request(&app, "POST", "/users", Some("application/json"), payload.clone()).await;
    assert_eq!(status, 201);
    assert_eq!(original, json!({"id":1,"name":"Alice","points":100,"version":0}));
    let (status, first) = request(&app, "PUT", "/users/1/points", Some("application/json"), json!({"points":120,"expected_version":0}).to_string()).await;
    assert_eq!(status, 200);
    assert_eq!(first, json!({"user_id":1,"points":120,"version":1}));
    let (status, stale) = request(&app, "PUT", "/users/1/points", Some("application/json"), json!({"points":80,"expected_version":0}).to_string()).await;
    assert_eq!(status, 409);
    assert_eq!(stale["error"], "version conflict; read the latest user before retrying");
    let (_, current) = request(&app, "GET", "/users/1", None, String::new()).await;
    assert_eq!((current["points"].as_i64(), current["version"].as_i64()), (Some(120), Some(1)));
    // B 要扣 20，读取新状态后重算为 100，而不是只换版本继续写旧结果 80。
    let (status, retried) = request(&app, "PUT", "/users/1/points", Some("application/json"), json!({"points":100,"expected_version":1}).to_string()).await;
    assert_eq!(status, 200);
    assert_eq!(retried, json!({"user_id":1,"points":100,"version":2}));
    let (status, replay) = request(&app, "POST", "/users", Some("application/json"), payload).await;
    assert_eq!(status, 201);
    assert_eq!(replay, original);
    let events = repo.list_outbox().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.points, 100);
    repo.close().await;
}

#[tokio::test]
async fn invalid_points_update_requests_leave_account_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("invalid.db")).await.unwrap());
    register(&repo).await;
    let app = api::app(Arc::clone(&repo));
    let cases = vec![
        ("/users/nope/points", Some("application/json"), json!({"points":120,"expected_version":0}).to_string(), 400),
        ("/users/0/points", Some("application/json"), json!({"points":120,"expected_version":0}).to_string(), 400),
        ("/users/99/points", Some("application/json"), json!({"points":120,"expected_version":0}).to_string(), 404),
        ("/users/1/points", None, json!({"points":120,"expected_version":0}).to_string(), 415),
        ("/users/1/points", Some("text/plain"), json!({"points":120,"expected_version":0}).to_string(), 415),
        ("/users/1/points", Some("application/json"), "null".into(), 400),
        ("/users/1/points", Some("application/json"), "[120,0]".into(), 400),
        ("/users/1/points", Some("application/json"), "{}".into(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":120}).to_string(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":null,"expected_version":0}).to_string(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":120,"expected_version":null}).to_string(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":1.5,"expected_version":0}).to_string(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":120,"expected_version":"0"}).to_string(), 400),
        ("/users/1/points", Some("application/json"), r#"{"points":120,"expected_version":9223372036854775808}"#.into(), 400),
        ("/users/1/points", Some("application/json"), r#"{"points":9223372036854775808,"expected_version":0}"#.into(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":120,"expected_version":0,"extra":1}).to_string(), 400),
        ("/users/1/points", Some("application/json"), r#"{"points":120,"expected_version":0} {}"#.into(), 400),
        ("/users/1/points", Some("application/json"), json!({"points":-1,"expected_version":0}).to_string(), 422),
        ("/users/1/points", Some("application/json"), json!({"points":120,"expected_version":-1}).to_string(), 422),
        ("/users/1/points", Some("application/json"), " ".repeat(1100)+"{}", 413),
    ];
    for (path, content_type, body, expected) in cases {
        let (status, response) = request(&app, "PUT", path, content_type, body.clone()).await;
        assert_eq!(status, expected, "path={path} body={body} response={response}");
        assert!(response["error"].is_string());
    }
    let method = app.oneshot(Request::builder().method("POST").uri("/users/1/points").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(method.status().as_u16(), 405);
    assert_eq!(method.headers()[header::ALLOW], "PUT");
    let current = repo.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((current.points, current.version), (100, 0));
    repo.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_version_across_pools_has_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("race.db");
    let first = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    let second = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    register(&first).await;
    let start = Arc::new(Barrier::new(32));
    let mut tasks = JoinSet::new();
    for index in 0..32 {
        let repo = if index % 2 == 0 { Arc::clone(&first) } else { Arc::clone(&second) };
        let start = Arc::clone(&start);
        tasks.spawn(async move { start.wait().await; repo.replace_points(1, 1000+index, 0).await });
    }
    let mut winner = None;
    let mut conflicts = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Ok(account) => { assert!(winner.is_none()); winner = Some(account); }
            Err(RepositoryError::VersionConflict) => conflicts += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!(conflicts, 31);
    let winner = winner.unwrap();
    let current = second.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((current.points, current.version, winner.version), (winner.points, 1, 1));
    assert_counts(&first).await;
    first.close().await;
    second.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_read_recalculate_retry_keeps_every_increment() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("retry.db");
    let first = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    let second = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    register(&first).await;
    let start = Arc::new(Barrier::new(16));
    let mut tasks = JoinSet::new();
    for index in 0..16 {
        let repo = if index % 2 == 0 { Arc::clone(&first) } else { Arc::clone(&second) };
        let start = Arc::clone(&start);
        tasks.spawn(async move {
            start.wait().await;
            for _ in 0..128 {
                let user = repo.find_by_id(1).await.unwrap().unwrap();
                match repo.replace_points(1, user.points+1, user.version).await {
                    Ok(_) => return,
                    Err(RepositoryError::VersionConflict) => tokio::task::yield_now().await,
                    Err(error) => panic!("unexpected error: {error:?}"),
                }
            }
            panic!("retry budget exhausted");
        });
    }
    while let Some(result) = tasks.join_next().await { result.unwrap(); }
    let current = first.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((current.points, current.version), (116, 16));
    first.close().await;
    second.close().await;
}

#[tokio::test]
async fn points_validation_and_version_limit() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("limits.db")).await.unwrap();
    register(&repo).await;
    assert!(matches!(repo.replace_points(99, 100, 0).await, Err(RepositoryError::UserNotFound)));
    assert!(matches!(repo.replace_points(1, -1, 0).await, Err(RepositoryError::InvalidUpdatedPoints)));
    assert!(matches!(repo.replace_points(1, 120, -1).await, Err(RepositoryError::InvalidVersion)));
    // 仅为边界测试直接设置版本，业务修改均通过 replace_points。
    sqlx::query("UPDATE points_accounts SET version = ? WHERE user_id = 1").bind(i64::MAX).execute(repo.pool()).await.unwrap();
    assert!(matches!(repo.replace_points(1, 120, i64::MAX).await, Err(RepositoryError::VersionExhausted)));
    let current = repo.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((current.points, current.version), (100, i64::MAX));
    repo.close().await;
}

#[tokio::test]
async fn failed_points_statement_keeps_version_and_successful_update_persists() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("persist.db");
    let repo = SqliteUserRepository::open(&path).await.unwrap();
    register(&repo).await;
    sqlx::raw_sql("CREATE TRIGGER reject_points BEFORE UPDATE ON points_accounts BEGIN SELECT RAISE(ABORT, 'simulated update failure'); END;")
        .execute(repo.pool()).await.unwrap();
    assert!(repo.replace_points(1, 120, 0).await.is_err());
    let unchanged = repo.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((unchanged.points, unchanged.version), (100, 0));
    sqlx::query("DROP TRIGGER reject_points").execute(repo.pool()).await.unwrap();
    assert_eq!(repo.replace_points(1, 120, 0).await.unwrap().version, 1);
    repo.close().await;
    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    let current = reopened.find_by_id(1).await.unwrap().unwrap();
    assert_eq!((current.points, current.version), (120, 1));
    let replay = reopened.create("outbox-key".into(), "Alice".into(), 100).await.unwrap();
    assert!(replay.replayed);
    assert_eq!((replay.user.points, replay.user.version), (100, 0));
    assert_counts(&reopened).await;
    reopened.close().await;
}
