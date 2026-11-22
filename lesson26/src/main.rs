mod demo;
use lesson26::server::{AnyError,start};

#[tokio::main]
async fn main()->Result<(),AnyError>{
    let args:Vec<_>=std::env::args().skip(1).collect();
    if args.iter().any(|arg|arg=="--demo"){
        let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
        return demo::run(&format!("data/http-demo-{stamp}.db")).await;
    }
    let option=|name:&str,default:&str|args.windows(2).find(|pair|pair[0]==name).map(|pair|pair[1].clone()).unwrap_or_else(||default.into());
    let path=option("--db","data/tasks.db");let addr=option("--addr","127.0.0.1:8081");
    let mut service=start(&path,&addr).await?;println!("Task API {} db={path:?}",service.url);
    tokio::select! {
        result=service.wait()=>result,
        signal=tokio::signal::ctrl_c()=>{signal?;service.shutdown();service.wait().await}
    }
}
