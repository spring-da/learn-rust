use super::*;
use std::sync::Arc;
use tokio::{sync::Barrier, task::JoinSet};

async fn assert_counts(repo: &SqliteUserRepository, users: i64, accounts: i64, records: i64) {
    let actual: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM points_accounts), \
         (SELECT COUNT(*) FROM idempotency_records)",
    )
    .fetch_one(&repo.pool)
    .await
    .unwrap();
    assert_eq!(actual, (users, accounts, records));
}

#[tokio::test]
async fn replay_persists_and_uses_original_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("idempotency.db");
    let repo = SqliteUserRepository::open(&path).await.unwrap();
    let first = repo.create("persist-key".into(), "O'Reilly".into(), 100).await.unwrap();
    assert!(!first.replayed);
    sqlx::query("UPDATE points_accounts SET points = 150 WHERE user_id = ?")
        .bind(first.user.id)
        .execute(&repo.pool)
        .await
        .unwrap();
    repo.close().await;

    let reopened = SqliteUserRepository::open(&path).await.unwrap();
    let replay = reopened.create("persist-key".into(), "O'Reilly".into(), 100).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.user, first.user);
    assert_eq!(reopened.find_by_id(first.user.id).await.unwrap().unwrap().points, 150);
    assert!(matches!(
        reopened.create("persist-key".into(), "O'Reilly".into(), 101).await,
        Err(RepositoryError::KeyMismatch)
    ));
    assert!(matches!(
        reopened.create("another-key".into(), "O'Reilly".into(), 100).await,
        Err(RepositoryError::NameExists)
    ));
    assert_counts(&reopened, 1, 1, 1).await;
    reopened.close().await;
}

#[tokio::test]
async fn failure_does_not_reserve_key() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("retry.db")).await.unwrap();
    assert!(matches!(
        repo.create("retry-key".into(), "Recoverable".into(), -1).await,
        Err(RepositoryError::InvalidPoints)
    ));
    assert_counts(&repo, 0, 0, 0).await;
    let created = repo.create("retry-key".into(), "Recoverable".into(), 50).await.unwrap();
    assert!(!created.replayed);
    let replay = repo.create("retry-key".into(), "Recoverable".into(), 50).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.user, created.user);
    assert_counts(&repo, 1, 1, 1).await;
    repo.close().await;
}

#[tokio::test]
async fn idempotency_write_failure_rolls_back_business_writes() {
    let directory = tempfile::tempdir().unwrap();
    let repo = SqliteUserRepository::open(&directory.path().join("atomic.db")).await.unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER reject_cache BEFORE INSERT ON idempotency_records \
         WHEN NEW.idempotency_key = 'fail-cache' \
         BEGIN SELECT RAISE(ABORT, 'simulated idempotency write failure'); END;",
    )
    .execute(&repo.pool)
    .await
    .unwrap();
    assert!(matches!(
        repo.create("fail-cache".into(), "Atomic".into(), 100).await,
        Err(RepositoryError::Database(_))
    ));
    // 检查三张物理表，不能只用 JOIN 判断是否留下半成品。
    assert_counts(&repo, 0, 0, 0).await;
    sqlx::query("DROP TRIGGER reject_cache").execute(&repo.pool).await.unwrap();
    let result = repo.create("fail-cache".into(), "Atomic".into(), 100).await.unwrap();
    assert!(!result.replayed);
    assert_counts(&repo, 1, 1, 1).await;
    repo.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_key_across_pools() {
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
            repo.create("concurrent-key".into(), "Concurrent".into(), 100).await
        });
    }
    let mut expected = None;
    let mut created = 0;
    let mut replays = 0;
    while let Some(outcome) = tasks.join_next().await {
        let result = outcome.unwrap().unwrap();
        if result.replayed { replays += 1; } else { created += 1; }
        if let Some(user) = &expected {
            assert_eq!(&result.user, user);
        } else {
            expected = Some(result.user);
        }
    }
    assert_eq!((created, replays), (1, 31));
    assert_counts(&first, 1, 1, 1).await;
    first.close().await;
    second.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_keys_still_respect_name_uniqueness() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unique.db");
    let first = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    let second = Arc::new(SqliteUserRepository::open(&path).await.unwrap());
    let start = Arc::new(Barrier::new(16));
    let mut tasks = JoinSet::new();
    for index in 0..16 {
        let repo = if index % 2 == 0 { Arc::clone(&first) } else { Arc::clone(&second) };
        let start = Arc::clone(&start);
        tasks.spawn(async move {
            start.wait().await;
            repo.create(format!("key-{index}"), "SameName".into(), 100).await
        });
    }
    let mut created = 0;
    let mut conflicts = 0;
    while let Some(outcome) = tasks.join_next().await {
        match outcome.unwrap() {
            Ok(result) => { assert!(!result.replayed); created += 1; }
            Err(RepositoryError::NameExists) => conflicts += 1,
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
    assert_eq!((created, conflicts), (1, 15));
    assert_counts(&first, 1, 1, 1).await;
    first.close().await;
    second.close().await;
}
