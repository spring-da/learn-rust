use serde::Serialize;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub points: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrationResult {
    pub user: User,
    pub replayed: bool,
}

#[derive(Debug)]
pub enum RepositoryError {
    NameExists,
    InvalidPoints,
    KeyMismatch,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(database_error) = error.as_database_error() {
            if database_error.is_unique_violation() {
                return Self::NameExists;
            }
            if database_error.is_check_violation() {
                return Self::InvalidPoints;
            }
        }
        Self::Database(error)
    }
}

pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;
        sqlx::raw_sql(include_str!("../schema.sql")).execute(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError> {
        let row: Option<(i64, String, i64)> = sqlx::query_as(
            "SELECT u.id, u.name, a.points FROM users AS u \
             JOIN points_accounts AS a ON a.user_id = u.id WHERE u.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, name, points)| User { id, name, points }))
    }

    pub async fn create(&self, key: String, name: String, initial_points: i64) -> Result<RegistrationResult, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let outcome: Result<RegistrationResult, RepositoryError> = async {
            let saved: Option<(String, i64, i64, String, i64)> = sqlx::query_as(
                "SELECT request_name, request_points, user_id, response_name, response_points \
                 FROM idempotency_records WHERE idempotency_key = ?",
            )
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((saved_name, saved_points, id, response_name, response_points)) = saved {
                if saved_name != name || saved_points != initial_points {
                    return Err(RepositoryError::KeyMismatch);
                }
                return Ok(RegistrationResult {
                    user: User { id, name: response_name, points: response_points },
                    replayed: true,
                });
            }

            let (id, created_name): (i64, String) = sqlx::query_as(
                "INSERT INTO users (name) VALUES (?) RETURNING id, name",
            )
            .bind(&name)
            .fetch_one(&mut *tx)
            .await?;
            let (points,): (i64,) = sqlx::query_as(
                "INSERT INTO points_accounts (user_id, points) VALUES (?, ?) RETURNING points",
            )
            .bind(id)
            .bind(initial_points)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO idempotency_records \
                 (idempotency_key, request_name, request_points, user_id, response_name, response_points) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&key)
            .bind(&name)
            .bind(initial_points)
            .bind(id)
            .bind(&created_name)
            .bind(points)
            .execute(&mut *tx)
            .await?;
            Ok(RegistrationResult { user: User { id, name: created_name, points }, replayed: false })
        }
        .await;

        match outcome {
            Ok(result) => {
                tx.commit().await?;
                Ok(result)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
