use crate::repository::{SqliteUserRepository, RepositoryError, User};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    repo: Arc<SqliteUserRepository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUserRequest {
    name: Option<String>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

enum ApiError {
    InvalidId,
    InvalidJson,
    InvalidName,
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

pub fn app(repo: Arc<SqliteUserRepository>) -> Router {
    Router::new()
        .route("/users", post(create_user).fallback(|| async { ApiError::MethodNotAllowed("POST") }))
        .route("/users/{id}", get(get_user).fallback(|| async { ApiError::MethodNotAllowed("GET, HEAD") }))
        .fallback(|| async { ApiError::RouteNotFound })
        .layer(DefaultBodyLimit::max(1024))
        .with_state(AppState { repo })
}

async fn create_user(
    State(state): State<AppState>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Response, ApiError> {
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
    let user = state.repo.create(name).await?;
    let location = format!("/users/{}", user.id);
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(user)).into_response())
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

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
