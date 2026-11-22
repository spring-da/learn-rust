use crate::{AppState, router};
use lesson24::Store;
use lesson25::{Clock, Policy, RealClock, Runner};
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use tokio::sync::watch;

pub type AnyError=Box<dyn std::error::Error+Send+Sync>;
pub struct Service { pub url:String, shutdown:watch::Sender<bool>, joined:tokio::task::JoinHandle<Result<(),String>> }
impl Service {
    pub fn shutdown(&self){let _=self.shutdown.send(true);}
    pub async fn wait(&mut self)->Result<(),AnyError>{(&mut self.joined).await?.map_err(Into::into)}
}
impl Drop for Service { fn drop(&mut self) { self.shutdown(); } }
pub async fn start(path:&str,addr:&str)->Result<Service,AnyError>{
    let store=Arc::new(Store::open(path,RealClock.now_ms(),64)?);
    let mut runner=Runner::start(store.clone(),Policy{base_ms:40,cap_ms:400},2)?;
    let listener=tokio::net::TcpListener::bind(addr).await?;
    let url=format!("http://{}",listener.local_addr()?);
    let( shutdown,mut receiver)=watch::channel(false);
    let ready=Arc::new(AtomicBool::new(true));
    let control=runner.control();
    let state=AppState{store:store.clone(),ready:ready.clone(),control:control.clone(),shutdown:shutdown.clone()};
    let joined=tokio::spawn(async move{
        let signal=async move{
            while !*receiver.borrow_and_update(){if receiver.changed().await.is_err(){break;}}
            ready.store(false,Ordering::Relaxed);control.request_stop();
        };
        let result=axum::serve(listener,router(state)).with_graceful_shutdown(signal).await;
        let runner_result=tokio::task::spawn_blocking(move||runner.stop()).await.map_err(|error|error.to_string())?;
        drop(store);
        result.map_err(|error|error.to_string())?;runner_result
    });
    Ok(Service{url,shutdown,joined})
}
