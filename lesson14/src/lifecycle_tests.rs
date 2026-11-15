use super::*;
use crate::{api, repository::SqliteUserRepository};
use axum::{body::Body, http::Request as HttpRequest};
use tokio::{io::AsyncWriteExt, task::JoinHandle};
use tower::ServiceExt;

async fn wait_active(lifecycle: &Lifecycle, want: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while lifecycle.state().active_requests != want { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
}

async fn start(repo: Arc<SqliteUserRepository>, grace: Duration) -> (Arc<Lifecycle>, String, JoinHandle<io::Result<ShutdownReport>>) {
    let lifecycle = Lifecycle::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = with_lifecycle(api::app(repo), Arc::clone(&lifecycle));
    let service = tokio::spawn(serve_until_shutdown(listener, app, Arc::clone(&lifecycle), grace));
    (lifecycle, url, service)
}

async fn counts(repo: &SqliteUserRepository, want: i64) {
    let actual: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts), (SELECT COUNT(*) FROM idempotency_records), (SELECT COUNT(*) FROM outbox_events)"
    ).fetch_one(repo.pool()).await.unwrap();
    assert_eq!(actual, (want, want, want, want));
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(4)).build().unwrap()
}

#[tokio::test]
async fn draining_rejects_new_work_and_readiness() {
    let lifecycle = Lifecycle::new();
    let active = lifecycle.enter().unwrap();
    let business = Router::new().route("/work", get(|| async { "must not run" }));
    let app = with_lifecycle(business, Arc::clone(&lifecycle));
    let ready = app.clone().oneshot(HttpRequest::builder().uri("/readyz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    lifecycle.request_shutdown();
    lifecycle.request_shutdown();
    let refused = app.clone().oneshot(HttpRequest::builder().uri("/work").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ready = app.oneshot(HttpRequest::builder().uri("/readyz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(tokio::time::timeout(Duration::from_millis(10), lifecycle.wait_drained()).await.is_err());
    drop(active);
    tokio::time::timeout(Duration::from_secs(1), lifecycle.wait_drained()).await.unwrap();
}

#[tokio::test]
async fn graceful_shutdown_completes_registration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("graceful.db");
    let repo = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    let (lifecycle, url, service) = start(Arc::clone(&repo), Duration::from_secs(2)).await;
    let request = client().post(format!("{url}/users"))
        .header("Idempotency-Key", "shutdown-key").header("X-Demo-Delay-Ms", "200")
        .json(&json!({"name":"Graceful","initial_points":100}));
    let pending = tokio::spawn(async move { request.send().await });
    wait_active(&lifecycle, 1).await;
    let shutdown = client().post(format!("{url}/demo/shutdown")).send().await.unwrap();
    assert_eq!(shutdown.status().as_u16(), 202);
    assert_eq!(pending.await.unwrap().unwrap().status().as_u16(), 201);
    let report = tokio::time::timeout(Duration::from_secs(4), service).await.unwrap().unwrap().unwrap();
    assert!(!report.forced);
    assert_eq!(lifecycle.state().active_requests, 0);
    counts(&repo, 1).await;
    repo.close().await;
    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    counts(&reopened, 1).await;
    assert_eq!(reopened.list_outbox().await.unwrap()[0].status, "pending");
    reopened.close().await;
}

#[tokio::test]
async fn grace_deadline_cancels_delayed_request_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("forced.db")).await.unwrap());
    let (lifecycle, url, service) = start(Arc::clone(&repo), Duration::from_millis(50)).await;
    let request = client().post(format!("{url}/users"))
        .header("Idempotency-Key", "shutdown-key").header("X-Demo-Delay-Ms", "2000")
        .json(&json!({"name":"Graceful","initial_points":100}));
    let pending = tokio::spawn(async move { request.send().await });
    wait_active(&lifecycle, 1).await;
    let start = std::time::Instant::now();
    lifecycle.request_shutdown();
    let report = tokio::time::timeout(Duration::from_secs(3), service).await.unwrap().unwrap().unwrap();
    assert!(report.forced);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(pending.await.unwrap().unwrap().status().as_u16(), 503);
    assert_eq!(lifecycle.state().active_requests, 0);
    counts(&repo, 0).await;
    repo.close().await;
}

#[tokio::test]
async fn grace_deadline_cancels_incomplete_request_body() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("body.db")).await.unwrap());
    let (lifecycle, url, service) = start(Arc::clone(&repo), Duration::from_millis(50)).await;
    let mut connection = tokio::net::TcpStream::connect(url.trim_start_matches("http://")).await.unwrap();
    connection.write_all(b"POST /users HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nIdempotency-Key: body-key\r\nContent-Length: 64\r\n\r\n{\"name\":").await.unwrap();
    wait_active(&lifecycle, 1).await;
    lifecycle.request_shutdown();
    let report = tokio::time::timeout(Duration::from_secs(3), service).await.unwrap().unwrap().unwrap();
    assert!(report.forced);
    assert_eq!(lifecycle.state().active_requests, 0);
    counts(&repo, 0).await;
    repo.close().await;
}

#[tokio::test]
async fn invalid_demo_delay_does_not_enter_business_handler() {
    let lifecycle = Lifecycle::new();
    let app = with_lifecycle(Router::new().route("/work", get(|| async { "must not run" })), Arc::clone(&lifecycle));
    for value in ["", "bad", "-1", "10001"] {
        let request = HttpRequest::builder().uri("/work").header("X-Demo-Delay-Ms", value).body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
        assert_eq!(lifecycle.state().active_requests, 0);
    }
    let request = HttpRequest::builder().uri("/work").header("X-Demo-Delay-Ms", "1").header("X-Demo-Delay-Ms", "2").body(Body::empty()).unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(lifecycle.state().active_requests, 0);
}
