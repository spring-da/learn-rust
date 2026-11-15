use super::*;
use crate::notifications::{self, NotificationRepository};
use axum::{Router, routing::post};
use std::path::Path;
use tokio::{sync::Notify, task::{JoinHandle, JoinSet}};

struct Receiver {
    repo: Arc<NotificationRepository>,
    url: String,
    server: JoinHandle<()>,
}

impl Receiver {
    async fn start(path: &Path, fail_first: bool, lose_ack: bool) -> Self {
        let repo = Arc::new(NotificationRepository::open(path).await.unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/notifications", listener.local_addr().unwrap());
        let app = notifications::router(Arc::clone(&repo), fail_first, lose_ack);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        Self { repo, url, server }
    }

    fn sender(&self) -> HttpEventSender { HttpEventSender::new(self.url.clone()).unwrap() }

    async fn close(&self) { self.server.abort(); self.repo.close().await; }
}

impl Drop for Receiver { fn drop(&mut self) { self.server.abort(); } }

async fn register(repo: &SqliteUserRepository) {
    repo.create("outbox-key".into(), "Alice".into(), 100).await.unwrap();
}

async fn one_outbox(repo: &SqliteUserRepository) -> OutboxItem {
    let mut items = repo.list_outbox().await.unwrap();
    assert_eq!(items.len(), 1);
    items.pop().unwrap()
}

#[tokio::test]
async fn outbox_insert_failure_rolls_back_all_writes() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("atomic.db")).await.unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_event BEFORE INSERT ON outbox_events BEGIN SELECT RAISE(ABORT, 'simulated outbox write failure'); END;")
        .execute(repo.pool()).await.unwrap();
    assert!(repo.create("outbox-key".into(), "Alice".into(), 100).await.is_err());
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts), (SELECT COUNT(*) FROM idempotency_records), (SELECT COUNT(*) FROM outbox_events)"
    ).fetch_one(repo.pool()).await.unwrap();
    assert_eq!(counts, (0, 0, 0, 0));
    sqlx::query("DROP TRIGGER reject_event").execute(repo.pool()).await.unwrap();
    register(&repo).await;
    let first = one_outbox(&repo).await;
    assert_eq!(first.status, "pending");
    assert_eq!(first.attempts, 0);
    assert_eq!(first.event.name, "Alice");
    assert_eq!(first.event.points, 100);
    assert_eq!(first.event.event_type, "UserRegistered");
    assert_eq!(first.event.event_id.len(), 32);
    register(&repo).await;
    assert_eq!(one_outbox(&repo).await.event, first.event);
    repo.close().await;
}

#[tokio::test]
async fn delivery_retries_only_when_due() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("outbox.db")).await.unwrap();
    let receiver = Receiver::start(&directory.path().join("notifications.db"), true, false).await;
    let sender = receiver.sender();
    register(&repo).await;
    assert!(dispatch_once(&repo, &sender, 1000, Duration::from_secs(2)).await.unwrap());
    let first = one_outbox(&repo).await;
    assert_eq!((first.status.as_str(), first.attempts, first.next_attempt_at), ("pending", 1, 3000));
    assert!(!first.last_error.is_empty());
    assert!(receiver.repo.list().await.unwrap().is_empty());
    assert!(!dispatch_once(&repo, &sender, 2999, Duration::from_secs(2)).await.unwrap());
    assert!(dispatch_once(&repo, &sender, 3000, Duration::from_secs(2)).await.unwrap());
    let last = one_outbox(&repo).await;
    assert_eq!((last.status.as_str(), last.attempts, last.next_attempt_at), ("sent", 2, 0));
    assert!(last.last_error.is_empty());
    assert_eq!(receiver.repo.list().await.unwrap().len(), 1);
    assert!(!dispatch_once(&repo, &sender, 4000, Duration::from_secs(2)).await.unwrap());
    receiver.close().await;
    repo.close().await;
}

#[tokio::test]
async fn lost_acknowledgement_survives_both_repositories_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let receiver_path = directory.path().join("notifications.db");
    let repo = SqliteUserRepository::open(&path).await.unwrap();
    let receiver = Receiver::start(&receiver_path, false, true).await;
    register(&repo).await;
    dispatch_once(&repo, &receiver.sender(), 1000, Duration::from_secs(1)).await.unwrap();
    let first = one_outbox(&repo).await;
    assert_eq!(first.status, "pending");
    assert_eq!(receiver.repo.list().await.unwrap().len(), 1);
    receiver.close().await;
    repo.close().await;

    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    let restarted = Receiver::start(&receiver_path, false, false).await;
    dispatch_once(&reopened, &restarted.sender(), 2000, Duration::from_secs(1)).await.unwrap();
    let last = one_outbox(&reopened).await;
    assert_eq!(last.event, first.event);
    assert_eq!((last.status.as_str(), last.attempts), ("sent", 2));
    assert_eq!(restarted.repo.list().await.unwrap().len(), 1);
    restarted.close().await;
    reopened.close().await;
}

#[tokio::test]
async fn mark_sent_failure_allows_safe_redelivery() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("outbox.db")).await.unwrap();
    let receiver = Receiver::start(&directory.path().join("notifications.db"), false, false).await;
    let sender = receiver.sender();
    register(&repo).await;
    sqlx::raw_sql("CREATE TRIGGER reject_mark BEFORE UPDATE ON outbox_events WHEN NEW.status = 'sent' BEGIN SELECT RAISE(ABORT, 'simulated crash before marking sent'); END;")
        .execute(repo.pool()).await.unwrap();
    assert!(dispatch_once(&repo, &sender, 1000, Duration::from_secs(1)).await.is_err());
    assert_eq!(one_outbox(&repo).await.status, "pending");
    assert_eq!(receiver.repo.list().await.unwrap().len(), 1);
    sqlx::query("DROP TRIGGER reject_mark").execute(repo.pool()).await.unwrap();
    dispatch_once(&repo, &sender, 2000, Duration::from_secs(1)).await.unwrap();
    assert_eq!(one_outbox(&repo).await.status, "sent");
    assert_eq!(receiver.repo.list().await.unwrap().len(), 1);
    receiver.close().await;
    repo.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn receiver_concurrent_duplicates_and_parameter_mismatch() {
    let directory = tempfile::tempdir().unwrap();
    let receiver = Receiver::start(&directory.path().join("notifications.db"), false, false).await;
    let event = RegistrationEvent { event_id: "0123456789abcdef0123456789abcdef".into(), event_type: "UserRegistered".into(), user_id: 1, name: "Alice".into(), points: 100 };
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().unwrap();
    let mut tasks = JoinSet::new();
    for _ in 0..16 {
        let client = client.clone();
        let url = receiver.url.clone();
        let event = event.clone();
        tasks.spawn(async move { client.post(url).json(&event).send().await.unwrap().status() });
    }
    while let Some(result) = tasks.join_next().await { assert_eq!(result.unwrap().as_u16(), 200); }
    assert_eq!(receiver.repo.list().await.unwrap(), vec![event.clone()]);
    let changed = RegistrationEvent { points: 101, ..event };
    assert_eq!(client.post(&receiver.url).json(&changed).send().await.unwrap().status().as_u16(), 409);
    receiver.close().await;
}

#[tokio::test]
async fn worker_cancellation_during_http() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("outbox.db")).await.unwrap());
    register(&repo).await;
    let entered = Arc::new(Notify::new());
    let notify = Arc::clone(&entered);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sender = HttpEventSender::new(format!("http://{}/notifications", listener.local_addr().unwrap())).unwrap();
    let app = Router::new().route("/notifications", post(move || {
        let notify = Arc::clone(&notify);
        async move { notify.notify_one(); std::future::pending::<()>().await; "unreachable" }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let (stop, shutdown) = watch::channel(false);
    let worker = tokio::spawn(run_worker(Arc::clone(&repo), sender, shutdown, Duration::from_secs(3600), Duration::from_secs(1)));
    tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), worker).await.unwrap().unwrap();
    assert_eq!(one_outbox(&repo).await.status, "pending");
    server.abort();
    repo.close().await;
}
