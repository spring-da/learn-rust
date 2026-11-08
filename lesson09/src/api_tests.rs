use super::*;
use axum::{body::{Body, to_bytes}, http::Request};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn test_router() -> (tempfile::TempDir, Arc<SqliteUserRepository>, Router) {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteUserRepository::open(&directory.path().join("api.db")).await.unwrap());
    let router = app(Arc::clone(&repo));
    (directory, repo, router)
}

async fn request(router: &Router, method: &str, path: &str, content_type: Option<&str>, body: String) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    router.clone().oneshot(builder.body(Body::from(body)).unwrap()).await.unwrap()
}

async fn json_body(response: Response) -> Value {
    assert!(response.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("application/json"));
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn create_read_and_conflict() {
    let (_directory, repo, router) = test_router().await;
    let created = request(&router, "POST", "/users", Some("application/json"), json!({"name":"  Alice  "}).to_string()).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()[header::LOCATION], "/users/1");
    assert_eq!(json_body(created).await, json!({"id":1,"name":"Alice"}));
    let fetched = request(&router, "GET", "/users/1", None, String::new()).await;
    assert_eq!(fetched.status(), StatusCode::OK);
    assert_eq!(json_body(fetched).await, json!({"id":1,"name":"Alice"}));
    let conflict = request(&router, "POST", "/users", Some("application/json"), json!({"name":"Alice"}).to_string()).await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(conflict).await, json!({"error":"name already exists"}));
    repo.close().await;
}

#[tokio::test]
async fn invalid_create_requests() {
    let (_directory, repo, router) = test_router().await;
    let cases = vec![
        (None, String::from(r#"{"name":"Alice"}"#), 415),
        (Some("text/plain"), String::from(r#"{"name":"Alice"}"#), 415),
        (Some("application/json"), String::new(), 400),
        (Some("application/json"), String::from(r#"{"name":"#), 400),
        (Some("application/json"), String::from("null"), 400),
        (Some("application/json"), String::from("[]"), 400),
        (Some("application/json"), String::from(r#"["Alice"]"#), 400),
        (Some("application/json"), json!({"name":123}).to_string(), 400),
        (Some("application/json"), json!({"name":"Alice","role":"admin"}).to_string(), 400),
        (Some("application/json"), String::from(r#"{"name":"Alice"} {"name":"Bob"}"#), 400),
        (Some("application/json"), String::from("{}"), 422),
        (Some("application/json"), json!({"name":null}).to_string(), 422),
        (Some("application/json"), json!({"name":"   "}).to_string(), 422),
        (Some("application/json"), json!({"name":"A"}).to_string(), 422),
        (Some("application/json"), json!({"name":"a".repeat(41)}).to_string(), 422),
        (Some("application/json"), json!({"name":"a".repeat(1500)}).to_string(), 413),
    ];
    for (content_type, body, status) in cases {
        let response = request(&router, "POST", "/users", content_type, body.clone()).await;
        assert_eq!(response.status().as_u16(), status, "content_type={content_type:?}, body={body}");
        assert!(json_body(response).await["error"].is_string());
    }
    let created = request(&router, "POST", "/users", Some("application/json"), json!({"name":"学习"}).to_string()).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(json_body(created).await, json!({"id":1,"name":"学习"}));
    repo.close().await;
}
