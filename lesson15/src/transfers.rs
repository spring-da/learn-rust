use crate::repository::{RepositoryError, SqliteUserRepository};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TransferReceipt {
    pub id: i64,
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub amount: i64,
    pub from_points: i64,
    pub to_points: i64,
    pub from_version: i64,
    pub to_version: i64,
}

#[derive(Debug)]
pub struct TransferResult {
    pub transfer: TransferReceipt,
    pub replayed: bool,
}

type TransferRow = (i64, i64, i64, i64, i64, i64, i64, i64);

fn from_row(row: TransferRow) -> TransferReceipt {
    let (id, from_user_id, to_user_id, amount, from_points, to_points, from_version, to_version) = row;
    TransferReceipt { id, from_user_id, to_user_id, amount, from_points, to_points, from_version, to_version }
}

impl SqliteUserRepository {
    pub async fn find_transfer(&self, id: i64) -> Result<Option<TransferReceipt>, RepositoryError> {
        let row: Option<TransferRow> = sqlx::query_as(
            "SELECT id, from_user_id, to_user_id, amount, from_points, to_points, from_version, to_version FROM transfers WHERE id = ?"
        ).bind(id).fetch_optional(self.pool()).await.map_err(RepositoryError::Database)?;
        Ok(row.map(from_row))
    }

    pub async fn transfer(&self, key: String, from_id: i64, to_id: i64, amount: i64) -> Result<TransferResult, RepositoryError> {
        if from_id <= 0 || to_id <= 0 || from_id == to_id || amount <= 0 { return Err(RepositoryError::InvalidTransfer); }
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await.map_err(RepositoryError::Database)?;
        let outcome: Result<TransferResult, RepositoryError> = async {
            let saved: Option<TransferRow> = sqlx::query_as(
                "SELECT id, from_user_id, to_user_id, amount, from_points, to_points, from_version, to_version FROM transfers WHERE idempotency_key = ?"
            ).bind(&key).fetch_optional(&mut *tx).await.map_err(RepositoryError::Database)?;
            if let Some(row) = saved {
                let receipt = from_row(row);
                if (receipt.from_user_id, receipt.to_user_id, receipt.amount) != (from_id, to_id, amount) {
                    return Err(RepositoryError::TransferKeyMismatch);
                }
                return Ok(TransferResult { transfer: receipt, replayed: true });
            }
            // 两个账户都通过同一事务连接读取；SQLite 此时已经持有写事务。
            let from: Option<(i64, i64)> = sqlx::query_as("SELECT points, version FROM points_accounts WHERE user_id = ?")
                .bind(from_id).fetch_optional(&mut *tx).await.map_err(RepositoryError::Database)?;
            let to: Option<(i64, i64)> = sqlx::query_as("SELECT points, version FROM points_accounts WHERE user_id = ?")
                .bind(to_id).fetch_optional(&mut *tx).await.map_err(RepositoryError::Database)?;
            let (from_points, from_version) = from.ok_or(RepositoryError::UserNotFound)?;
            let (to_points, to_version) = to.ok_or(RepositoryError::UserNotFound)?;
            if from_points < amount { return Err(RepositoryError::InsufficientPoints); }
            // checked_add 明确拒绝溢出，不依赖 SQLite 将超范围整数运算转成其他类型。
            if to_points.checked_add(amount).is_none() { return Err(RepositoryError::PointsOverflow); }
            if from_version == i64::MAX || to_version == i64::MAX { return Err(RepositoryError::VersionExhausted); }
            let (new_from_points, new_from_version): (i64, i64) = sqlx::query_as(
                "UPDATE points_accounts SET points = points - ?, version = version + 1 \
                 WHERE user_id = ? AND points >= ? AND version = ? RETURNING points, version"
            ).bind(amount).bind(from_id).bind(amount).bind(from_version)
                .fetch_one(&mut *tx).await.map_err(RepositoryError::Database)?;
            let (new_to_points, new_to_version): (i64, i64) = sqlx::query_as(
                "UPDATE points_accounts SET points = points + ?, version = version + 1 \
                 WHERE user_id = ? AND points <= ? AND version = ? RETURNING points, version"
            ).bind(amount).bind(to_id).bind(i64::MAX-amount).bind(to_version)
                .fetch_one(&mut *tx).await.map_err(RepositoryError::Database)?;
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO transfers \
                 (idempotency_key, from_user_id, to_user_id, amount, from_points, to_points, from_version, to_version) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id"
            ).bind(&key).bind(from_id).bind(to_id).bind(amount)
                .bind(new_from_points).bind(new_to_points).bind(new_from_version).bind(new_to_version)
                .fetch_one(&mut *tx).await.map_err(RepositoryError::Database)?;
            Ok(TransferResult {
                transfer: TransferReceipt { id, from_user_id: from_id, to_user_id: to_id, amount,
                    from_points: new_from_points, to_points: new_to_points,
                    from_version: new_from_version, to_version: new_to_version },
                replayed: false,
            })
        }.await;
        match outcome {
            Ok(result) => { tx.commit().await.map_err(RepositoryError::Database)?; Ok(result) }
            Err(error) => { tx.rollback().await.map_err(RepositoryError::Database)?; Err(error) }
        }
    }
}

#[cfg(test)]
#[path = "transfers_tests.rs"]
mod tests;
