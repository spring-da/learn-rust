use axum::{Router,body::{Body,to_bytes},http::{Request,StatusCode}};
use lesson24::Store;
use lesson25::{Policy,Runner};
use lesson26::{AppState,router};
use std::sync::{Arc,atomic::AtomicBool};
use tower::ServiceExt;
use serde_json::{Value,json};

struct Fixture{app:Router,runner:Runner,_store:Arc<Store>,_receiver:tokio::sync::watch::Receiver<bool>,_directory:tempfile::TempDir}
impl Fixture{
    fn new(capacity:usize)->Self{
        let directory=tempfile::tempdir().unwrap();let store=Arc::new(Store::open(directory.path().join("queue.db"),0,capacity).unwrap());
        let runner=Runner::start(store.clone(),Policy{base_ms:40,cap_ms:400},1).unwrap();let(shutdown,receiver)=tokio::sync::watch::channel(false);
        let state=AppState{store:store.clone(),ready:Arc::new(AtomicBool::new(true)),control:runner.control(),shutdown};
        Self{app:router(state),runner,_store:store,_receiver:receiver,_directory:directory}
    }
    async fn request(&self,method:&str,path:&str,key:Option<&str>,body:&str)->(StatusCode,Value,axum::http::HeaderMap){
        let mut request=Request::builder().method(method).uri(path).header("Content-Type","application/json");if let Some(key)=key{request=request.header("Idempotency-Key",key);}
        let response=self.app.clone().oneshot(request.body(Body::from(body.to_owned())).unwrap()).await.unwrap();let status=response.status();let headers=response.headers().clone();let bytes=to_bytes(response.into_body(),usize::MAX).await.unwrap();
        (status,serde_json::from_slice(&bytes).unwrap(),headers)
    }
}
impl Drop for Fixture{fn drop(&mut self){self.runner.stop().unwrap();}}
fn body()->String{json!({"kind":"uppercase","payload":"hello","max_attempts":3,"delay_ms":60000}).to_string()}

#[tokio::test]
async fn submission_replay_and_conflict(){
    let fixture=Fixture::new(1);let(status,first,headers)=fixture.request("POST","/tasks",Some("key"),&body()).await;assert_eq!(status,201);assert_eq!(headers["idempotency-replayed"],"false");
    let(status,replayed,headers)=fixture.request("POST","/tasks",Some("key"),&body()).await;assert_eq!(status,201);assert_eq!(headers["idempotency-replayed"],"true");assert_eq!(first["id"],replayed["id"]);
    assert_eq!(fixture.request("POST","/tasks",Some("key"),&body().replace("hello","different")).await.0,409);
}

#[tokio::test]
async fn strict_input_and_status_mapping(){
    let fixture=Fixture::new(64);
    let cases=[(None,body(),400),(Some("bad key"),body(),400),(Some("good"),"null".into(),400),(Some("good"),"{}".into(),400),
        (Some("good"),json!({"kind":"uppercase","payload":"hello","max_attempts":0}).to_string(),422),
        (Some("good"),json!({"kind":"unknown","payload":"hello","max_attempts":1}).to_string(),422),
        (Some("good"),body().replace("60000","null"),400),
        (Some("good"),body().replace("\"delay_ms\":60000","\"extra\":1"),400),
        (Some("good"),format!("{} {{}}",body()),400),(Some("good"),"x".repeat(4097),413)];
    for(key,body,expected)in cases{let(status,response,_)=fixture.request("POST","/tasks",key,&body).await;assert_eq!(status.as_u16(),expected,"{body} {response}");}
    assert_eq!(fixture.request("GET","/tasks/0",None,"").await.0,400);assert_eq!(fixture.request("GET","/tasks/999",None,"").await.0,404);
    let response=fixture.app.clone().oneshot(Request::builder().method("POST").uri("/tasks").header("Idempotency-Key","type").body(Body::from(body())).unwrap()).await.unwrap();assert_eq!(response.status(),415);
    let response=fixture.app.clone().oneshot(Request::builder().method("POST").uri("/tasks").header("Content-Type","application/json").header("Idempotency-Key","a").header("Idempotency-Key","b").body(Body::from(body())).unwrap()).await.unwrap();assert_eq!(response.status(),400);
}

#[tokio::test]
async fn cancellation_frees_capacity_and_keeps_terminal_state(){
    let fixture=Fixture::new(1);let(_,task,headers)=fixture.request("POST","/tasks",Some("one"),&body()).await;
    assert_eq!(fixture.request("POST","/tasks",Some("two"),&body()).await.0,429);
    let path=format!("{}/cancel",headers["location"].to_str().unwrap());let(status,cancelled,_)=fixture.request("POST",&path,None,"").await;assert_eq!(status,202);assert_eq!(cancelled["status"],"cancelled");
    assert_eq!(fixture.request("POST",&path,None,"").await.0,409);assert_eq!(fixture.request("POST","/tasks",Some("two"),&body()).await.0,201);
    assert_eq!(fixture.request("GET",&format!("/tasks/{}",task["id"]),None,"").await.1["id"],task["id"]);
}

#[tokio::test]
async fn concurrent_idempotent_requests_create_one_task(){
    let fixture=Fixture::new(64);let mut handles=Vec::new();
    for _ in 0..16{let app=fixture.app.clone();handles.push(tokio::spawn(async move{app.oneshot(Request::builder().method("POST").uri("/tasks").header("Content-Type","application/json").header("Idempotency-Key","same").body(Body::from(body())).unwrap()).await.unwrap()}));}
    let mut created=0;
    for handle in handles{let response=handle.await.unwrap();assert_eq!(response.status(),201);if response.headers()["idempotency-replayed"]=="false"{created+=1};let bytes=to_bytes(response.into_body(),usize::MAX).await.unwrap();let value:Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(value["id"],1);}
    assert_eq!(created,1);
}

#[tokio::test]
async fn draining_rejects_new_work(){
    let fixture=Fixture::new(64);assert_eq!(fixture.request("POST","/demo/shutdown",None,"").await.0,202);
    assert_eq!(fixture.request("GET","/readyz",None,"").await.0,503);assert_eq!(fixture.request("POST","/tasks",Some("new"),&body()).await.0,503);
}
