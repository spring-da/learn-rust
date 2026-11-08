use serde::Serialize;
use sqlx::{SqlitePool, sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous}};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
}

#[derive(Debug)]
pub enum RepositoryError {
    NameExists,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for RepositoryError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(database_error) = error.as_database_error() {
            if database_error.is_unique_violation() {
                return Self::NameExists;
            }
        }
        Self::Database(error)
    }
}

pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    // 调用方先创建父目录，SQLite 可以按需创建数据库文件。
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
        sqlx::query(include_str!("../schema.sql")).execute(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError> {
        let row: Option<(i64, String)> = sqlx::query_as("SELECT id, name FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(id, name)| User { id, name }))
    }

    pub async fn create(&self, name: String) -> Result<User, RepositoryError> {
        let (id, name): (i64, String) = sqlx::query_as(
            "INSERT INTO users (name) VALUES (?) RETURNING id, name"
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Ok(User { id, name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::{sync::Barrier, task::JoinSet};

    #[tokio::test]
    async fn persistence_and_bound_parameters() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("users with spaces.db");
        let repo = SqliteUserRepository::open(&path).await.unwrap();
        let user = repo.create(String::from("O'Reilly")).await.unwrap();
        repo.close().await;
        let reopened = SqliteUserRepository::open(&path).await.unwrap();
        assert_eq!(reopened.find_by_id(user.id).await.unwrap(), Some(user.clone()));
        assert!(matches!(reopened.create(user.name).await, Err(RepositoryError::NameExists)));
        assert_eq!(reopened.find_by_id(user.id + 10000).await.unwrap(), None);
        reopened.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unique_constraint_across_pools() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shared.db");
        let first = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
        let second = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
        let start = Arc::new(Barrier::new(32));
        let mut tasks = JoinSet::new();
        for index in 0..32 {
            let repo = if index % 2 == 0 { Arc::clone(&first) } else { Arc::clone(&second) };
            let start = Arc::clone(&start);
            tasks.spawn(async move {
                start.wait().await;
                repo.create(String::from("Concurrent")).await
            });
        }
        let mut created = 0;
        let mut conflicts = 0;
        while let Some(outcome) = tasks.join_next().await {
            match outcome.unwrap() {
                Ok(_) => created += 1,
                Err(RepositoryError::NameExists) => conflicts += 1,
                Err(error) => panic!("unexpected database error: {error:?}"),
            }
        }
        assert_eq!(created, 1);
        assert_eq!(conflicts, 31);
        first.close().await;
        second.close().await;
    }
}
