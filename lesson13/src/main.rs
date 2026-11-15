mod api;
mod lifecycle;
mod notifications;
mod outbox;
mod repository;

use lifecycle::Lifecycle;
use notifications::NotificationRepository;
use outbox::HttpEventSender;
use repository::SqliteUserRepository;
use std::{path::PathBuf, sync::Arc, time::Duration};

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| fallback.into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grace_ms: u64 = env_or("SHUTDOWN_GRACE_MS", "5000").parse()?;
    if !(1..=10000).contains(&grace_ms) { return Err("SHUTDOWN_GRACE_MS must be 1 to 10000".into()); }
    let path = PathBuf::from(env_or("DB_PATH", "data/shutdown.db"));
    let notification_path = PathBuf::from(env_or("NOTIFY_DB_PATH", "data/notifications.db"));
    for path in [&path, &notification_path] {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { std::fs::create_dir_all(parent)?; }
    }
    let repo = Arc::new(SqliteUserRepository::open(&path).await?);
    let notifications = Arc::new(NotificationRepository::open(&notification_path).await?);
    let listener = tokio::net::TcpListener::bind(env_or("HTTP_ADDR", "127.0.0.1:8081")).await?;
    let address = listener.local_addr()?;
    let notify_url = env_or("NOTIFY_URL", &format!("http://127.0.0.1:{}/notifications", address.port()));
    let lifecycle = Lifecycle::new();
    let business = api::app(Arc::clone(&repo)).merge(notifications::router(Arc::clone(&notifications), env_or("DEMO_FAIL_FIRST", "1") == "1", env_or("DEMO_LOSE_ACK_ONCE", "0") == "1"));
    let app = lifecycle::with_lifecycle(business, Arc::clone(&lifecycle));
    let worker = if env_or("WORKER_ENABLED", "1") == "1" {
        let worker_repo = Arc::clone(&repo);
        let sender = HttpEventSender::new(notify_url)?;
        let shutdown = lifecycle.subscribe_shutdown();
        Some(tokio::spawn(async move { outbox::run_worker(worker_repo, sender, shutdown, Duration::from_secs(1), Duration::from_secs(2)).await; }))
    } else { None };
    let signal_lifecycle = Arc::clone(&lifecycle);
    let signal_task = tokio::spawn(async move {
        if let Err(error) = tokio::signal::ctrl_c().await { eprintln!("shutdown signal: {error}"); }
        signal_lifecycle.request_shutdown();
    });
    println!("database: {}; notification database: {}", path.display(), notification_path.display());
    println!("Rust shutdown API listening at http://{address}");
    println!("grace: {grace_ms}ms; worker enabled: {}", env_or("WORKER_ENABLED", "1"));
    let result = lifecycle::serve_until_shutdown(listener, app, Arc::clone(&lifecycle), Duration::from_millis(grace_ms)).await;
    signal_task.abort();
    let _ = signal_task.await;
    let worker_result = if let Some(worker) = worker { worker.await } else { Ok(()) };
    // 先等业务处理与 worker 结束，再关闭连接池；SQLx 也会等归还连接。
    notifications.close().await;
    repo.close().await;
    let report = result?;
    worker_result?;
    println!("shutdown complete: forced={} active={}; worker stopped; databases closed", report.forced, lifecycle.state().active_requests);
    Ok(())
}
