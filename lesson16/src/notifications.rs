use crate::{outbox::RegistrationEvent, repository::open_pool};
use axum::{Json, Router, extract::{DefaultBodyLimit, State, rejection::JsonRejection}, http::StatusCode, response::{IntoResponse, Response}, routing::get};
use serde_json::json;
use sqlx::SqlitePool;
use std::{path::Path, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub struct NotificationRepository { pool: SqlitePool }

pub enum AcceptError { Mismatch, Database(sqlx::Error) }
impl From<sqlx::Error> for AcceptError {
    fn from(error: sqlx::Error) -> Self { Self::Database(error) }
}

impl NotificationRepository {
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        let pool = open_pool(path, "CREATE TABLE IF NOT EXISTS notifications (event_id TEXT PRIMARY KEY NOT NULL, user_id INTEGER NOT NULL, name TEXT NOT NULL, points INTEGER NOT NULL);").await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) { self.pool.close().await; }

    pub async fn accept(&self, event: &RegistrationEvent) -> Result<bool, AcceptError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let outcome: Result<bool, AcceptError> = async {
            let result = sqlx::query("INSERT INTO notifications (event_id, user_id, name, points) VALUES (?, ?, ?, ?) ON CONFLICT(event_id) DO NOTHING")
                .bind(&event.event_id).bind(event.user_id).bind(&event.name).bind(event.points)
                .execute(&mut *tx).await?;
            let replayed = result.rows_affected() == 0;
            if replayed {
                let (user_id, name, points): (i64, String, i64) = sqlx::query_as("SELECT user_id, name, points FROM notifications WHERE event_id = ?")
                    .bind(&event.event_id).fetch_one(&mut *tx).await?;
                if (user_id, name.as_str(), points) != (event.user_id, event.name.as_str(), event.points) { return Err(AcceptError::Mismatch); }
            }
            Ok(replayed)
        }.await;
        match outcome {
            Ok(replayed) => { tx.commit().await?; Ok(replayed) }
            Err(error) => { tx.rollback().await?; Err(error) }
        }
    }

    pub async fn list(&self) -> Result<Vec<RegistrationEvent>, sqlx::Error> {
        let rows: Vec<(String, i64, String, i64)> = sqlx::query_as("SELECT event_id, user_id, name, points FROM notifications ORDER BY rowid LIMIT 100")
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|(event_id, user_id, name, points)| RegistrationEvent {
            event_id, event_type: "UserRegistered".into(), user_id, name, points,
        }).collect())
    }
}

struct NotificationState { repo: Arc<NotificationRepository>, fail_first: AtomicBool, lose_ack_once: AtomicBool }

pub fn router(repo: Arc<NotificationRepository>, fail_first: bool, lose_ack_once: bool) -> Router {
    Router::new().route("/notifications", get(list).post(receive))
        .layer(DefaultBodyLimit::max(2048))
        .with_state(Arc::new(NotificationState { repo, fail_first: AtomicBool::new(fail_first), lose_ack_once: AtomicBool::new(lose_ack_once) }))
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}

fn valid_event(event: &RegistrationEvent) -> bool {
    event.event_id.len() == 32 && event.event_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && event.event_type == "UserRegistered" && event.user_id > 0 && event.points >= 0 && (2..=40).contains(&event.name.chars().count())
}

async fn receive(State(state): State<Arc<NotificationState>>, payload: Result<Json<serde_json::Value>, JsonRejection>) -> Response {
    let event = match payload {
        Ok(Json(value)) if value.is_object() => serde_json::from_value::<RegistrationEvent>(value).ok(),
        _ => None,
    };
    let Some(event) = event.filter(valid_event) else { return error(StatusCode::BAD_REQUEST, "invalid notification event"); };
    if state.fail_first.swap(false, Ordering::SeqCst) {
        return error(StatusCode::SERVICE_UNAVAILABLE, "simulated failure before processing");
    }
    match state.repo.accept(&event).await {
        Ok(replayed) => {
            if state.lose_ack_once.swap(false, Ordering::SeqCst) {
                return error(StatusCode::SERVICE_UNAVAILABLE, "simulated failure after processing");
            }
            Json(json!({"replayed":replayed})).into_response()
        }
        Err(AcceptError::Mismatch) => error(StatusCode::CONFLICT, "event id was used with different parameters"),
        Err(AcceptError::Database(cause)) => { eprintln!("notification repository: {cause}"); error(StatusCode::SERVICE_UNAVAILABLE, "notification repository unavailable") }
    }
}

async fn list(State(state): State<Arc<NotificationState>>) -> Response {
    match state.repo.list().await {
        Ok(events) => Json(events).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "notification repository unavailable"),
    }
}
