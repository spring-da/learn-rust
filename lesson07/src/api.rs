use crate::repository::{MemoryUserRepository, User};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};

const QUERY_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Clone)]
struct AppState {
    repo: Arc<MemoryUserRepository>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

enum ApiError {
    InvalidId,
    UserNotFound,
    RepositoryUnavailable,
    QueryTimeout,
    MethodNotAllowed,
    RouteNotFound,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::InvalidId => (StatusCode::BAD_REQUEST, "id must be a positive 64-bit integer"),
            Self::UserNotFound => (StatusCode::NOT_FOUND, "user not found"),
            Self::RepositoryUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "repository unavailable"),
            Self::QueryTimeout => (StatusCode::GATEWAY_TIMEOUT, "query deadline exceeded"),
            Self::MethodNotAllowed => (StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
            Self::RouteNotFound => (StatusCode::NOT_FOUND, "route not found"),
        };
        let mut response = (status, Json(ErrorBody { error: message })).into_response();
        if status == StatusCode::METHOD_NOT_ALLOWED {
            response
                .headers_mut()
                .insert(header::ALLOW, "GET, HEAD".parse().expect("valid Allow header"));
        }
        response
    }
}

pub fn app(repo: Arc<MemoryUserRepository>) -> Router {
    Router::new()
        .route("/users/{id}", get(get_user))
        .fallback(|| async { ApiError::RouteNotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .with_state(AppState { repo })
}

async fn get_user(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<User>, ApiError> {
    let id = match raw_id.parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => return Err(ApiError::InvalidId),
    };

    match tokio::time::timeout(QUERY_TIMEOUT, state.repo.find_by_id(id)).await {
        Err(_) => Err(ApiError::QueryTimeout),
        Ok(Err(error)) => {
            eprintln!("lookup id={id} failed: {error}");
            Err(ApiError::RepositoryUnavailable)
        }
        Ok(Ok(None)) => Err(ApiError::UserNotFound),
        Ok(Ok(Some(user))) => Ok(Json(user)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    #[tokio::test]
    async fn http_contract() {
        let router = app(Arc::new(MemoryUserRepository::new()));
        let cases = [
            ("GET", "/users/1", 200, ""),
            ("GET", "/users/abc", 400, "id must be a positive 64-bit integer"),
            ("GET", "/users/0", 400, "id must be a positive 64-bit integer"),
            ("GET", "/users/-1", 400, "id must be a positive 64-bit integer"),
            ("GET", "/users/9223372036854775808", 400, "id must be a positive 64-bit integer"),
            ("GET", "/users/404", 404, "user not found"),
            ("GET", "/users/500", 503, "repository unavailable"),
            ("GET", "/users/99", 504, "query deadline exceeded"),
            ("POST", "/users/1", 405, "method not allowed"),
            ("GET", "/missing", 404, "route not found"),
        ];
        for (method, path, status, message) in cases {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .expect("valid test request"),
                )
                .await
                .expect("router returns a response");
            assert_eq!(response.status().as_u16(), status, "{method} {path}");
            assert!(
                response.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            );
            if status == 405 {
                assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
            }
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            if message.is_empty() {
                assert_eq!(value, json!({"id": 1, "name": "Spring"}));
            } else {
                assert_eq!(value, json!({"error": message}), "{method} {path}");
            }
        }
    }
}
