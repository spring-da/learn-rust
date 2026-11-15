use crate::repository::{SqliteUserRepository, RepositoryError, User};
use crate::{cache::{CacheStats,TtlCache},transfers::TransferReceipt};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use std::{sync::{Arc,atomic::{AtomicU64,Ordering}},time::Instant};

#[derive(Clone)]
struct AppState {
    repo: Arc<SqliteUserRepository>,
    receipts: Arc<TtlCache<i64,TransferReceipt>>,
    receipt_loads: Arc<AtomicU64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUserRequest {
    name: Option<String>,
    initial_points: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacePointsRequest {
    points: i64,
    expected_version: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTransferRequest {
    from_user_id: i64,
    to_user_id: i64,
    amount: i64,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

enum ApiError {
    InvalidId,
    InvalidJson,
    InvalidName,
    InvalidPoints,
    InvalidUpdatedPoints,
    InvalidVersion,
    VersionConflict,
    VersionExhausted,
    InvalidTransfer,
    TransferKeyMismatch,
    InsufficientPoints,
    PointsOverflow,
    TransferNotFound,
    InvalidKey,
    KeyMismatch,
    NameConflict,
    UnsupportedMediaType,
    BodyTooLarge,
    UserNotFound,
    RepositoryUnavailable,
    RouteNotFound,
    MethodNotAllowed(&'static str),
}

impl From<RepositoryError> for ApiError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::NameExists => Self::NameConflict,
            RepositoryError::InvalidPoints => Self::InvalidPoints,
            RepositoryError::KeyMismatch => Self::KeyMismatch,
            RepositoryError::UserNotFound => Self::UserNotFound,
            RepositoryError::VersionConflict => Self::VersionConflict,
            RepositoryError::VersionExhausted => Self::VersionExhausted,
            RepositoryError::InvalidVersion => Self::InvalidVersion,
            RepositoryError::InvalidUpdatedPoints => Self::InvalidUpdatedPoints,
            RepositoryError::InvalidTransfer => Self::InvalidTransfer,
            RepositoryError::TransferKeyMismatch => Self::TransferKeyMismatch,
            RepositoryError::InsufficientPoints => Self::InsufficientPoints,
            RepositoryError::PointsOverflow => Self::PointsOverflow,
            RepositoryError::Database(error) => {
                eprintln!("database operation failed: {error}");
                Self::RepositoryUnavailable
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message, allow) = match self {
            Self::InvalidId => (StatusCode::BAD_REQUEST, "id must be a positive 64-bit integer", None),
            Self::InvalidJson => (StatusCode::BAD_REQUEST, "invalid JSON body", None),
            Self::InvalidName => (StatusCode::UNPROCESSABLE_ENTITY, "name must contain 2 to 40 characters", None),
            Self::InvalidPoints => (StatusCode::UNPROCESSABLE_ENTITY, "initial points must be non-negative", None),
            Self::InvalidUpdatedPoints => (StatusCode::UNPROCESSABLE_ENTITY, "points must be non-negative", None),
            Self::InvalidVersion => (StatusCode::UNPROCESSABLE_ENTITY, "expected_version must be non-negative", None),
            Self::VersionConflict => (StatusCode::CONFLICT, "version conflict; read the latest user before retrying", None),
            Self::VersionExhausted => (StatusCode::CONFLICT, "account version cannot be advanced", None),
            Self::InvalidTransfer => (StatusCode::UNPROCESSABLE_ENTITY, "accounts must be distinct positive IDs and amount must be positive", None),
            Self::TransferKeyMismatch => (StatusCode::CONFLICT, "transfer idempotency key was used with different parameters", None),
            Self::InsufficientPoints => (StatusCode::CONFLICT, "insufficient points", None),
            Self::PointsOverflow => (StatusCode::CONFLICT, "destination points would exceed int64", None),
            Self::TransferNotFound => (StatusCode::NOT_FOUND, "transfer not found", None),
            Self::InvalidKey => (StatusCode::BAD_REQUEST, "Idempotency-Key must be 1-64 ASCII letters, digits, '-' or '_'", None),
            Self::KeyMismatch => (StatusCode::CONFLICT, "idempotency key was used with different parameters", None),
            Self::NameConflict => (StatusCode::CONFLICT, "name already exists", None),
            Self::UnsupportedMediaType => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type must be application/json", None),
            Self::BodyTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "request body too large", None),
            Self::UserNotFound => (StatusCode::NOT_FOUND, "user not found", None),
            Self::RepositoryUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "repository unavailable", None),
            Self::RouteNotFound => (StatusCode::NOT_FOUND, "route not found", None),
            Self::MethodNotAllowed(allow) => (StatusCode::METHOD_NOT_ALLOWED, "method not allowed", Some(allow)),
        };
        let mut response = (status, Json(ErrorBody { error: message })).into_response();
        if let Some(allow) = allow {
            response.headers_mut().insert(header::ALLOW, allow.parse().expect("valid Allow header"));
        }
        response
    }
}

#[cfg(test)]
pub fn app(repo: Arc<SqliteUserRepository>) -> Router {
    let cache=TtlCache::new(64,std::time::Duration::from_secs(2)).expect("valid default cache configuration");
    app_with_cache(repo,Arc::new(cache))
}

pub fn app_with_cache(repo:Arc<SqliteUserRepository>,receipts:Arc<TtlCache<i64,TransferReceipt>>)->Router {
    Router::new()
        .route("/users", post(create_user).fallback(|| async { ApiError::MethodNotAllowed("POST") }))
        .route("/cache/stats",get(cache_stats).fallback(|| async { ApiError::MethodNotAllowed("GET, HEAD") }))
        .route("/cache/receipts/{id}",delete(delete_receipt_cache).fallback(|| async { ApiError::MethodNotAllowed("DELETE") }))
        .route("/transfers", post(create_transfer).fallback(|| async { ApiError::MethodNotAllowed("POST") }))
        .route("/transfers/{id}", get(get_transfer).fallback(|| async { ApiError::MethodNotAllowed("GET, HEAD") }))
        .route("/outbox", get(list_outbox).fallback(|| async { ApiError::MethodNotAllowed("GET, HEAD") }))
        .route("/users/{id}", get(get_user).fallback(|| async { ApiError::MethodNotAllowed("GET, HEAD") }))
        .route("/users/{id}/points", put(replace_points).fallback(|| async { ApiError::MethodNotAllowed("PUT") }))
        .fallback(|| async { ApiError::RouteNotFound })
        .layer(DefaultBodyLimit::max(1024))
        .with_state(AppState { repo,receipts,receipt_loads:Arc::new(AtomicU64::new(0)) })
}

async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = parse_idempotency_key(&headers)?;
    let Json(value) = payload.map_err(|error| match error.status() {
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ApiError::UnsupportedMediaType,
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::BodyTooLarge,
        _ => ApiError::InvalidJson,
    })?;
    // 显式限定请求为对象，再按 DTO 解析字段，避免接受结构体的序列表达。
    if !value.is_object() {
        return Err(ApiError::InvalidJson);
    }
    let input: CreateUserRequest = serde_json::from_value(value).map_err(|_| ApiError::InvalidJson)?;
    let name = input.name.unwrap_or_default().trim().to_owned();
    if !(2..=40).contains(&name.chars().count()) {
        return Err(ApiError::InvalidName);
    }
    let result = state.repo.create(key, name, input.initial_points.unwrap_or(0)).await?;
    let location = format!("/users/{}", result.user.id);
    let headers = [
        (header::LOCATION, location),
        (HeaderName::from_static("idempotency-replayed"), result.replayed.to_string()),
    ];
    Ok((StatusCode::CREATED, headers, Json(result.user)).into_response())
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let mut values = headers.get_all("idempotency-key").iter();
    let value = values.next().ok_or(ApiError::InvalidKey)?;
    if values.next().is_some() {
        return Err(ApiError::InvalidKey);
    }
    let key = value.to_str().map_err(|_| ApiError::InvalidKey)?;
    if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(ApiError::InvalidKey);
    }
    Ok(key.to_owned())
}

async fn get_user(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<User>, ApiError> {
    let id = match raw_id.parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => return Err(ApiError::InvalidId),
    };
    let user = state.repo.find_by_id(id).await?.ok_or(ApiError::UserNotFound)?;
    Ok(Json(user))
}

async fn list_outbox(State(state): State<AppState>) -> Result<Json<Vec<crate::outbox::OutboxItem>>, ApiError> {
    state.repo.list_outbox().await.map(Json).map_err(|_| ApiError::RepositoryUnavailable)
}

async fn replace_points(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<crate::points::PointsAccount>, ApiError> {
    let id = raw_id.parse::<i64>().ok().filter(|id| *id > 0).ok_or(ApiError::InvalidId)?;
    let Json(value) = payload.map_err(|error| match error.status() {
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ApiError::UnsupportedMediaType,
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::BodyTooLarge,
        _ => ApiError::InvalidJson,
    })?;
    if !value.is_object() { return Err(ApiError::InvalidJson); }
    let input: ReplacePointsRequest = serde_json::from_value(value).map_err(|_| ApiError::InvalidJson)?;
    let account = state.repo.replace_points(id, input.points, input.expected_version).await?;
    Ok(Json(account))
}

async fn create_transfer(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Response, ApiError> {
    let key = parse_idempotency_key(&headers)?;
    let Json(value) = payload.map_err(|error| match error.status() {
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ApiError::UnsupportedMediaType,
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::BodyTooLarge,
        _ => ApiError::InvalidJson,
    })?;
    if !value.is_object() { return Err(ApiError::InvalidJson); }
    let input: CreateTransferRequest = serde_json::from_value(value).map_err(|_| ApiError::InvalidJson)?;
    let result = state.repo.transfer(key, input.from_user_id, input.to_user_id, input.amount).await?;
    let headers = [
        (header::LOCATION, format!("/transfers/{}", result.transfer.id)),
        (HeaderName::from_static("idempotency-replayed"), result.replayed.to_string()),
    ];
    Ok((StatusCode::CREATED, headers, Json(result.transfer)).into_response())
}

async fn get_transfer(State(state): State<AppState>, Path(raw_id): Path<String>) -> Result<Response, ApiError> {
    let id = raw_id.parse::<i64>().ok().filter(|id| *id > 0).ok_or(ApiError::InvalidId)?;
    if let Some(receipt)=state.receipts.get_at(&id,Instant::now()) {
        return Ok(([("x-receipt-cache","HIT")],Json(receipt)).into_response());
    }
    state.receipt_loads.fetch_add(1,Ordering::Relaxed);
    let receipt = state.repo.find_transfer(id).await?.ok_or(ApiError::TransferNotFound)?;
    // 缓存是查询优化；极端时钟范围错误不影响成功的数据库读取。
    let _=state.receipts.put_at(id,receipt.clone(),Instant::now());
    Ok(([("x-receipt-cache","MISS")],Json(receipt)).into_response())
}

#[derive(Serialize)]
struct ReceiptCacheStats {
    #[serde(flatten)]
    cache:CacheStats,
    database_loads:u64,
}

async fn cache_stats(State(state):State<AppState>)->Json<ReceiptCacheStats> {
    Json(ReceiptCacheStats {cache:state.receipts.stats_at(Instant::now()),database_loads:state.receipt_loads.load(Ordering::Relaxed)})
}

async fn delete_receipt_cache(State(state):State<AppState>,Path(raw_id):Path<String>)->Result<StatusCode,ApiError> {
    let id=raw_id.parse::<i64>().ok().filter(|id|*id>0).ok_or(ApiError::InvalidId)?;
    state.receipts.delete(&id);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
