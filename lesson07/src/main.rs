mod api;
mod repository;

use repository::MemoryUserRepository;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::var("HTTP_ADDR").unwrap_or_else(|_| String::from("127.0.0.1:8081"));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = api::app(Arc::new(MemoryUserRepository::new()));
    println!("Rust user API listening at http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
