mod api;
mod notifications;
mod outbox;
mod repository;

use notifications::NotificationRepository;
use outbox::HttpEventSender;
use repository::SqliteUserRepository;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::watch;

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| fallback.into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env_or("DB_PATH", "data/outbox.db"));
    let notification_path = PathBuf::from(env_or("NOTIFY_DB_PATH", "data/notifications.db"));
    for path in [&path, &notification_path] {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { std::fs::create_dir_all(parent)?; }
    }
    let repo = Arc::new(SqliteUserRepository::open(&path).await?);
    let notifications = Arc::new(NotificationRepository::open(&notification_path).await?);
    let listener = tokio::net::TcpListener::bind(env_or("HTTP_ADDR", "127.0.0.1:8081")).await?;
    let address = listener.local_addr()?;
    let notify_url = env_or("NOTIFY_URL", &format!("http://127.0.0.1:{}/notifications", address.port()));
    let enabled = env_or("WORKER_ENABLED", "1") == "1";
    let app = api::app(Arc::clone(&repo)).merge(notifications::router(Arc::clone(&notifications), env_or("DEMO_FAIL_FIRST", "1") == "1", env_or("DEMO_LOSE_ACK_ONCE", "0") == "1"));
    let (stop, shutdown) = watch::channel(false);
    let worker = if enabled {
        let worker_repo = Arc::clone(&repo);
        let sender = HttpEventSender::new(notify_url.clone())?;
        // move 把依赖所有权移进任务，Arc 让 API 和 worker 共享仓库。
        Some(tokio::spawn(async move { outbox::run_worker(worker_repo, sender, shutdown, Duration::from_secs(1), Duration::from_secs(2)).await; }))
    } else { None };
    println!("database: {}; notification database: {}", path.display(), notification_path.display());
    println!("Rust outbox API listening at http://{address}");
    println!("worker enabled: {enabled}; notification URL: {notify_url}");
    let signal_stop = stop.clone();
    let result = axum::serve(listener, app).with_graceful_shutdown(async move {
        if let Err(error) = tokio::signal::ctrl_c().await { eprintln!("shutdown signal: {error}"); }
        let _ = signal_stop.send(true);
    }).await;
    let _ = stop.send(true);
    if let Some(worker) = worker { worker.await?; }
    repo.close().await;
    notifications.close().await;
    result?;
    Ok(())
}
