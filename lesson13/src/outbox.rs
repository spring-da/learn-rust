use crate::repository::SqliteUserRepository;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationEvent {
    pub event_id: String,
    pub event_type: String,
    pub user_id: i64,
    pub name: String,
    pub points: i64,
}

#[derive(Debug, Serialize)]
pub struct OutboxItem {
    pub event: RegistrationEvent,
    pub status: String,
    pub attempts: i64,
    pub next_attempt_at: i64,
    pub last_error: String,
}

type OutboxRow = (String, i64, String, i64, String, i64, i64, String);

fn from_row(row: OutboxRow) -> OutboxItem {
    let (event_id, user_id, name, points, status, attempts, next_attempt_at, last_error) = row;
    OutboxItem {
        event: RegistrationEvent { event_id, event_type: "UserRegistered".into(), user_id, name, points },
        status, attempts, next_attempt_at, last_error,
    }
}

impl SqliteUserRepository {
    pub async fn list_outbox(&self) -> Result<Vec<OutboxItem>, sqlx::Error> {
        let rows: Vec<OutboxRow> = sqlx::query_as("SELECT event_id, user_id, name, points, status, attempts, next_attempt_at, last_error FROM outbox_events ORDER BY rowid LIMIT 100")
            .fetch_all(self.pool()).await?;
        Ok(rows.into_iter().map(from_row).collect())
    }

    async fn next_event(&self, now: i64) -> Result<Option<OutboxItem>, sqlx::Error> {
        let row: Option<OutboxRow> = sqlx::query_as(
            "SELECT event_id, user_id, name, points, status, attempts, next_attempt_at, last_error FROM outbox_events WHERE status = 'pending' AND next_attempt_at <= ? ORDER BY rowid LIMIT 1"
        ).bind(now).fetch_optional(self.pool()).await?;
        Ok(row.map(from_row))
    }
}

pub struct HttpEventSender {
    client: reqwest::Client,
    url: String,
}

impl HttpEventSender {
    pub fn new(url: String) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build()?;
        Ok(Self { client, url })
    }

    async fn send(&self, event: &RegistrationEvent) -> Result<(), String> {
        let response = self.client.post(&self.url).json(event).send().await.map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("notification endpoint returned HTTP {}", response.status().as_u16()));
        }
        Ok(())
    }
}

// now 为 Unix 毫秒，测试可以直接推进逻辑时间，避免等待重试定时器。
pub async fn dispatch_once(repo: &SqliteUserRepository, sender: &HttpEventSender, now: i64, retry_delay: Duration) -> Result<bool, sqlx::Error> {
    let Some(item) = repo.next_event(now).await? else { return Ok(false); };
    // SELECT 已完成；等待 HTTP 时没有持有数据库事务。
    if let Err(error) = sender.send(&item.event).await {
        sqlx::query("UPDATE outbox_events SET attempts = attempts + 1, next_attempt_at = ?, last_error = ? WHERE event_id = ? AND status = 'pending'")
            .bind(now + retry_delay.as_millis() as i64).bind(&error).bind(&item.event.event_id)
            .execute(repo.pool()).await?;
        eprintln!("event {}: delivery failed, retry scheduled: {error}", item.event.event_id);
    } else {
        sqlx::query("UPDATE outbox_events SET status = 'sent', attempts = attempts + 1, next_attempt_at = 0, last_error = '' WHERE event_id = ? AND status = 'pending'")
            .bind(&item.event.event_id).execute(repo.pool()).await?;
        println!("event {}: delivery confirmed", item.event.event_id);
    }
    Ok(true)
}

pub async fn run_worker(repo: Arc<SqliteUserRepository>, sender: HttpEventSender, mut shutdown: watch::Receiver<bool>, interval: Duration, retry_delay: Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() { return; }
        tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            _ = ticker.tick() => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock before epoch").as_millis() as i64;
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => return,
                    result = dispatch_once(&repo, &sender, now, retry_delay) => {
                        if let Err(error) = result { eprintln!("outbox worker: {error}"); }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
