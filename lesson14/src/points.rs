use crate::repository::{RepositoryError, SqliteUserRepository};
use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PointsAccount {
    pub user_id: i64,
    pub points: i64,
    pub version: i64,
}

impl SqliteUserRepository {
    pub async fn replace_points(&self, id: i64, points: i64, expected_version: i64) -> Result<PointsAccount, RepositoryError> {
        if points < 0 { return Err(RepositoryError::InvalidUpdatedPoints); }
        if expected_version < 0 { return Err(RepositoryError::InvalidVersion); }
        // 比较版本、替换积分、递增版本在同一条 SQL 中完成。
        let row: Option<(i64, i64, i64)> = sqlx::query_as(
            "UPDATE points_accounts SET points = ?, version = version + 1 \
             WHERE user_id = ? AND version = ? AND version < 9223372036854775807 \
             RETURNING user_id, points, version"
        ).bind(points).bind(id).bind(expected_version).fetch_optional(self.pool()).await
            .map_err(RepositoryError::Database)?;
        if let Some((user_id, points, version)) = row { return Ok(PointsAccount { user_id, points, version }); }
        // 补查仅用于错误分类，写入是否成立已经由上面的 SQL 决定。
        let current = self.find_by_id(id).await?.ok_or(RepositoryError::UserNotFound)?;
        if current.version == i64::MAX && expected_version == current.version { return Err(RepositoryError::VersionExhausted); }
        Err(RepositoryError::VersionConflict)
    }
}

#[cfg(test)]
#[path = "points_tests.rs"]
mod tests;
