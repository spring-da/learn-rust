pub mod server;
use axum::{Json, Router, extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection}, http::{HeaderMap, StatusCode, header}, response::{IntoResponse, Response}, routing::{get, post}};
use lesson24::{Error, Spec, Store, validate_key};
use lesson25::{Clock, Control, RealClock};
use serde_json::json;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::watch;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>, pub ready: Arc<AtomicBool>, pub control: Control, pub shutdown: watch::Sender<bool>,
}
impl AppState { fn accepting(&self) -> bool { self.ready.load(Ordering::Relaxed) && self.control.error().is_none() } }

pub fn router(state: AppState) -> Router {
    Router::new().route("/readyz",get(ready))
        .route("/tasks",get(list).post(submit))
        .route("/tasks/{id}",get(find))
        .route("/tasks/{id}/cancel",post(cancel))
        .route("/demo/shutdown",post(shutdown))
        .layer(DefaultBodyLimit::max(4096)).with_state(state)
}

pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError { fn into_response(self) -> Response { (self.0,Json(json!({"error":self.1}))).into_response() } }
impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        let status=match &error { Error::Invalid=>StatusCode::UNPROCESSABLE_ENTITY,Error::NotFound=>StatusCode::NOT_FOUND,Error::Conflict=>StatusCode::CONFLICT,Error::Full=>StatusCode::TOO_MANY_REQUESTS,_=>StatusCode::INTERNAL_SERVER_ERROR };
        let text=if status==StatusCode::INTERNAL_SERVER_ERROR {"database operation failed".into()}else{error.to_string()};Self(status,text)
    }
}
async fn blocking<T: Send+'static>(store: Arc<Store>, work: impl FnOnce(&Store)->Result<T,Error>+Send+'static) -> Result<T,ApiError> {
    tokio::task::spawn_blocking(move||work(&store)).await.map_err(|_|ApiError(StatusCode::INTERNAL_SERVER_ERROR,"database worker failed".into()))?.map_err(ApiError::from)
}
fn task_id(raw:&str)->Result<i64,ApiError>{raw.parse::<i64>().ok().filter(|id|*id>0).ok_or_else(||ApiError(StatusCode::BAD_REQUEST,"invalid task id".into()))}
fn draining()->ApiError{ApiError(StatusCode::SERVICE_UNAVAILABLE,"service is draining".into())}

async fn ready(State(state):State<AppState>)->impl IntoResponse{
    if state.accepting(){(StatusCode::OK,Json(json!({"status":"ready"})))}else{(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"status":"draining"})))}
}
async fn shutdown(State(state):State<AppState>)->impl IntoResponse{
    state.ready.store(false,Ordering::Relaxed);let _=state.shutdown.send(true);
    (StatusCode::ACCEPTED,Json(json!({"status":"stopping"})))
}
async fn list(State(state):State<AppState>)->Result<Json<Vec<lesson24::Task>>,ApiError>{Ok(Json(blocking(state.store,|store|store.list()).await?))}
async fn find(State(state):State<AppState>,Path(raw):Path<String>)->Result<Json<lesson24::Task>,ApiError>{let id=task_id(&raw)?;Ok(Json(blocking(state.store,move|store|store.get(id)).await?))}

async fn submit(State(state):State<AppState>,headers:HeaderMap,payload:Result<Json<Spec>,JsonRejection>)->Result<Response,ApiError>{
    if !state.accepting(){return Err(draining());}
    let keys:Vec<_>=headers.get_all("idempotency-key").iter().collect();
    if keys.len()!=1{return Err(ApiError(StatusCode::BAD_REQUEST,"invalid Idempotency-Key".into()));}
    let key=keys[0].to_str().map_err(|_|ApiError(StatusCode::BAD_REQUEST,"invalid Idempotency-Key".into()))?.to_owned();
    validate_key(&key).map_err(|_|ApiError(StatusCode::BAD_REQUEST,"invalid Idempotency-Key".into()))?;
    let Json(spec)=payload.map_err(|rejection|{
        let status=match rejection.status(){StatusCode::PAYLOAD_TOO_LARGE=>StatusCode::PAYLOAD_TOO_LARGE,StatusCode::UNSUPPORTED_MEDIA_TYPE=>StatusCode::UNSUPPORTED_MEDIA_TYPE,_=>StatusCode::BAD_REQUEST};ApiError(status,"invalid JSON body or fields".into())
    })?;
    let (task,replayed)=blocking(state.store,move|store|store.submit(&key,&spec,RealClock.now_ms())).await?;
    Ok((StatusCode::CREATED,[(header::LOCATION,format!("/tasks/{}",task.id)),(axum::http::HeaderName::from_static("idempotency-replayed"),replayed.to_string())],Json(task)).into_response())
}
async fn cancel(State(state):State<AppState>,Path(raw):Path<String>)->Result<Response,ApiError>{
    if !state.accepting(){return Err(draining());}
    let id=task_id(&raw)?;let(task,accepted)=blocking(state.store,move|store|store.cancel(id)).await?;
    if !accepted{return Err(ApiError(StatusCode::CONFLICT,"task is terminal or cancellation was already requested".into()));}
    Ok((StatusCode::ACCEPTED,Json(task)).into_response())
}
