use lesson26::server::{AnyError,start};
use reqwest::{Client,Method};
use serde_json::{Value,json};
use std::time::Duration;

async fn request(client:&Client,base:&str,method:Method,path:&str,key:Option<&str>,body:Option<&Value>,expected:u16)->Result<(Value,reqwest::header::HeaderMap),AnyError>{
    let mut request=client.request(method,format!("{base}{path}"));
    if let Some(key)=key{request=request.header("Idempotency-Key",key);}
    if let Some(body)=body{request=request.json(body);}
    let response=request.send().await?;let status=response.status().as_u16();let headers=response.headers().clone();let text=response.text().await?;
    if status!=expected{return Err(format!("{path}: status={status} expected={expected} body={text}").into());}
    Ok((serde_json::from_str(&text)?,headers))
}
async fn wait(client:&Client,base:&str,id:i64)->Result<Value,AnyError>{
    let start=std::time::Instant::now();
    loop{
        let(task,_)=request(client,base,Method::GET,&format!("/tasks/{id}"),None,None,200).await?;
        if matches!(task["status"].as_str(),Some("succeeded"|"failed"|"cancelled")){return Ok(task);}
        if start.elapsed()>Duration::from_secs(5){return Err("task timed out".into());}
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
pub async fn run(path:&str)->Result<(),AnyError>{
    let mut service=start(path,"127.0.0.1:0").await?;
    let client=Client::builder().no_proxy().timeout(Duration::from_secs(3)).build()?;
    let spec=json!({"kind":"flaky","payload":"hello HTTP 中文","max_attempts":3,"fail_until":2});
    let(task,headers)=request(&client,&service.url,Method::POST,"/tasks",Some("demo-key"),Some(&spec),201).await?;
    assert_eq!(headers["idempotency-replayed"],"false");
    let(replayed,headers)=request(&client,&service.url,Method::POST,"/tasks",Some("demo-key"),Some(&spec),201).await?;
    assert_eq!(task["id"],replayed["id"]);assert_eq!(headers["idempotency-replayed"],"true");
    let mut changed=spec.clone();changed["payload"]=json!("different");request(&client,&service.url,Method::POST,"/tasks",Some("demo-key"),Some(&changed),409).await?;
    let cancel_spec=json!({"kind":"uppercase","payload":"cancel me","max_attempts":2,"delay_ms":60000});
    let(cancel,_)=request(&client,&service.url,Method::POST,"/tasks",Some("cancel-key"),Some(&cancel_spec),201).await?;
    request(&client,&service.url,Method::POST,&format!("/tasks/{}/cancel",cancel["id"]),None,None,202).await?;
    let completed=wait(&client,&service.url,task["id"].as_i64().unwrap()).await?;
    assert_eq!(completed["status"],"succeeded");assert_eq!(completed["attempts"],3);assert_eq!(completed["result"],"HELLO HTTP 中文");
    let resume_spec=json!({"kind":"uppercase","payload":"resume later","max_attempts":3,"delay_ms":1000});
    let(resume,_)=request(&client,&service.url,Method::POST,"/tasks",Some("resume-key"),Some(&resume_spec),201).await?;
    request(&client,&service.url,Method::POST,"/demo/shutdown",None,None,202).await?;
    service.wait().await?;
    let mut service=start(path,"127.0.0.1:0").await?;
    let resumed=wait(&client,&service.url,resume["id"].as_i64().unwrap()).await?;
    assert_eq!(resumed["status"],"succeeded");assert_eq!(resumed["result"],"RESUME LATER");
    request(&client,&service.url,Method::POST,"/tasks",Some("demo-key"),Some(&spec),201).await?;
    request(&client,&service.url,Method::POST,"/demo/shutdown",None,None,202).await?;
    service.wait().await?;
    println!("HTTP demo passed: retries=3 idempotent=true cancel=true restart=true db={path:?}");
    Ok(())
}
