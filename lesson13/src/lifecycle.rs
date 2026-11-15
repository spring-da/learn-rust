use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use serde_json::json;
use std::{io, sync::{Arc, Mutex}, time::Duration};
use tokio::{net::TcpListener, sync::watch};

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ServiceState {
    pub draining: bool,
    pub active_requests: usize,
}

pub struct Lifecycle {
    state: Mutex<ServiceState>,
    state_changed: watch::Sender<ServiceState>,
    stop: watch::Sender<bool>,
    force: watch::Sender<bool>,
}

impl Lifecycle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ServiceState::default()),
            state_changed: watch::channel(ServiceState::default()).0,
            stop: watch::channel(false).0,
            force: watch::channel(false).0,
        })
    }

    pub fn state(&self) -> ServiceState { *self.state.lock().unwrap() }

    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> { self.stop.subscribe() }

    pub fn request_shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.draining = true;
        self.state_changed.send_replace(*state);
        self.stop.send_replace(true);
    }

    fn enter(self: &Arc<Self>) -> Option<ActiveRequest> {
        let mut state = self.state.lock().unwrap();
        if state.draining { return None; }
        state.active_requests += 1;
        self.state_changed.send_replace(*state);
        Some(ActiveRequest(Arc::clone(self)))
    }

    async fn wait_drained(&self) {
        let mut changes = self.state_changed.subscribe();
        loop {
            let state = *changes.borrow_and_update();
            if state.draining && state.active_requests == 0 { return; }
            if changes.changed().await.is_err() { return; }
        }
    }
}

struct ActiveRequest(Arc<Lifecycle>);

impl Drop for ActiveRequest {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.active_requests -= 1;
        self.0.state_changed.send_replace(*state);
    }
}

pub async fn wait_true(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() { return; }
        if receiver.changed().await.is_err() { return; }
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

fn delay(request: &Request) -> Result<Duration, ()> {
    let mut values = request.headers().get_all("x-demo-delay-ms").iter();
    let Some(value) = values.next() else { return Ok(Duration::ZERO); };
    if values.next().is_some() { return Err(()); }
    let milliseconds = value.to_str().map_err(|_| ())?.parse::<i64>().map_err(|_| ())?;
    if !(0..=10000).contains(&milliseconds) { return Err(()); }
    Ok(Duration::from_millis(milliseconds as u64))
}

async fn protect(State(lifecycle): State<Arc<Lifecycle>>, request: Request, next: Next) -> Response {
    let Some(active) = lifecycle.enter() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service is draining");
    };
    let delay = match delay(&request) {
        Ok(delay) => delay,
        Err(()) => return error(StatusCode::BAD_REQUEST, "X-Demo-Delay-Ms must be an integer from 0 to 10000"),
    };
    let mut force = lifecycle.force.subscribe();
    // 只有宽限期结束才触发 force；最初的 stop 只停止接收新工作。
    let response = tokio::select! {
        biased;
        _ = wait_true(&mut force) => error(StatusCode::SERVICE_UNAVAILABLE, "request cancelled during shutdown"),
        response = async {
            if !delay.is_zero() {
                println!("request {} {}: demo delay {}ms started", request.method(), request.uri().path(), delay.as_millis());
                tokio::time::sleep(delay).await;
            }
            next.run(request).await
        } => response,
    };
    // force 分支胜出时，上面的处理 Future 已被 drop，ActiveRequest 也会可靠减少计数。
    drop(active);
    response
}

pub fn with_lifecycle(business: Router, lifecycle: Arc<Lifecycle>) -> Router {
    let admin = Router::new()
        .route("/demo/state", get(state))
        .route("/demo/shutdown", post(shutdown))
        .route("/readyz", get(ready))
        .with_state(Arc::clone(&lifecycle));
    business.layer(middleware::from_fn_with_state(lifecycle, protect)).merge(admin)
}

async fn state(State(lifecycle): State<Arc<Lifecycle>>) -> Json<ServiceState> { Json(lifecycle.state()) }

async fn shutdown(State(lifecycle): State<Arc<Lifecycle>>) -> impl IntoResponse {
    lifecycle.request_shutdown();
    (StatusCode::ACCEPTED, Json(json!({"status":"draining"})))
}

async fn ready(State(lifecycle): State<Arc<Lifecycle>>) -> Response {
    if lifecycle.state().draining { return error(StatusCode::SERVICE_UNAVAILABLE, "service is draining"); }
    Json(json!({"status":"ready"})).into_response()
}

pub struct ShutdownReport { pub forced: bool }

pub async fn serve_until_shutdown(listener: TcpListener, app: Router, lifecycle: Arc<Lifecycle>, grace: Duration) -> io::Result<ShutdownReport> {
    let mut server_stop = lifecycle.subscribe_shutdown();
    let mut observe_stop = lifecycle.subscribe_shutdown();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { wait_true(&mut server_stop).await; })
            .await
    });
    let result = tokio::select! {
        result = &mut server => Some(result),
        _ = wait_true(&mut observe_stop) => None,
    };
    lifecycle.request_shutdown();
    println!("shutdown: draining active={} grace={}ms", lifecycle.state().active_requests, grace.as_millis());
    let mut forced = false;
    let result = match result {
        Some(result) => result,
        None => match tokio::time::timeout(grace, &mut server).await {
            Ok(result) => result,
            Err(_) => {
                forced = true;
                println!("shutdown: grace ended, cancelling active request futures");
                // 取消发生在每个业务请求的 middleware，不能只 abort 顶层 serve 任务。
                lifecycle.force.send_replace(true);
                server.await
            }
        },
    };
    if lifecycle.state().active_requests > 0 { lifecycle.force.send_replace(true); }
    lifecycle.wait_drained().await;
    result.map_err(io::Error::other)??;
    Ok(ShutdownReport { forced })
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
