use super::*;
use std::sync::Arc;
use tokio::{sync::Barrier, task::JoinSet};

async fn assert_table_counts(repo: &SqliteUserRepository, want_users: i64, want_accounts: i64) {
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts)",
    )
    .fetch_one(&repo.pool)
    .await
    .unwrap();
    assert_eq!(counts, (want_users, want_accounts));
}

#[tokio::test]
async fn commit_persists_both_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("registration.db");
    let repo = SqliteUserRepository::open(&path).await.unwrap();
    let user = repo.create(String::from("O'Reilly"), 100).await.unwrap();
    assert_eq!(user.points, 100);
    assert_table_counts(&repo, 1, 1).await;
    repo.close().await;

    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    assert_eq!(reopened.find_by_id(user.id).await.unwrap(), Some(user.clone()));
    assert!(matches!(reopened.create(user.name, 50).await, Err(RepositoryError::NameExists)));
    assert_table_counts(&reopened, 1, 1).await;
    reopened.close().await;
}

#[tokio::test]
async fn second_statement_failure_rolls_back_and_allows_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rollback.db");
    let repo = SqliteUserRepository::open(&path).await.unwrap();
    assert!(matches!(repo.create(String::from("Recoverable"), -1).await, Err(RepositoryError::InvalidPoints)));
    // 直接检查两个物理表，避免 JOIN 隐藏未回滚的用户行。
    assert_table_counts(&repo, 0, 0).await;
    repo.close().await;

    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    assert_table_counts(&reopened, 0, 0).await;
    let user = reopened.create(String::from("Recoverable"), 50).await.unwrap();
    assert_eq!(user.points, 50);
    assert_table_counts(&reopened, 1, 1).await;
    reopened.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_registrations_across_pools() {
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
            repo.create(String::from("Concurrent"), 100).await
        });
    }
    let mut created = 0;
    let mut conflicts = 0;
    while let Some(outcome) = tasks.join_next().await {
        match outcome.unwrap() {
            Ok(_) => created += 1,
            Err(RepositoryError::NameExists) => conflicts += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!((created, conflicts), (1, 31));
    assert_table_counts(&first, 1, 1).await;
    first.close().await;
    second.close().await;
}
