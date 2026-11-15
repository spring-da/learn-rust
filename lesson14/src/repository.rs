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
    pub version: i64,
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
    UserNotFound,
    VersionConflict,
    VersionExhausted,
    InvalidVersion,
    InvalidUpdatedPoints,
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
        let pool = open_pool(path, include_str!("../schema.sql")).await?;
        Ok(Self { pool })
    }

    pub(crate) fn pool(&self) -> &SqlitePool { &self.pool }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError> {
        let row: Option<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT u.id, u.name, a.points, a.version FROM users AS u \
             JOIN points_accounts AS a ON a.user_id = u.id WHERE u.id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id, name, points, version)| User { id, name, points, version }))
    }

    pub async fn create(&self, key: String, name: String, initial_points: i64) -> Result<RegistrationResult, RepositoryError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let outcome: Result<RegistrationResult, RepositoryError> = async {
            let saved: Option<(String, i64, i64, String, i64, i64)> = sqlx::query_as(
                "SELECT request_name, request_points, user_id, response_name, response_points, response_version \
                 FROM idempotency_records WHERE idempotency_key = ?",
            )
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((saved_name, saved_points, id, response_name, response_points, response_version)) = saved {
                if saved_name != name || saved_points != initial_points {
                    return Err(RepositoryError::KeyMismatch);
                }
                return Ok(RegistrationResult {
                    user: User { id, name: response_name, points: response_points, version: response_version },
                    replayed: true,
                });
            }

            let (id, created_name): (i64, String) = sqlx::query_as(
                "INSERT INTO users (name) VALUES (?) RETURNING id, name",
            )
            .bind(&name)
            .fetch_one(&mut *tx)
            .await?;
            let (points, version): (i64, i64) = sqlx::query_as(
                "INSERT INTO points_accounts (user_id, points) VALUES (?, ?) RETURNING points, version",
            )
            .bind(id)
            .bind(initial_points)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO idempotency_records \
                 (idempotency_key, request_name, request_points, user_id, response_name, response_points, response_version) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&key)
            .bind(&name)
            .bind(initial_points)
            .bind(id)
            .bind(&created_name)
            .bind(points)
            .bind(version)
            .execute(&mut *tx)
            .await?;
            // 重放分支已经返回；只有首次注册在同一事务中保存事件。
            sqlx::query("INSERT INTO outbox_events (user_id, name, points) VALUES (?, ?, ?)")
                .bind(id)
                .bind(&created_name)
                .bind(points)
                .execute(&mut *tx)
                .await
                .map_err(RepositoryError::Database)?;
            Ok(RegistrationResult { user: User { id, name: created_name, points, version }, replayed: false })
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

pub(crate) async fn open_pool(path: &Path, schema: &'static str) -> Result<SqlitePool, sqlx::Error> {
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
    sqlx::raw_sql(schema).execute(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;
