mod api;
mod repository;

use repository::SqliteUserRepository;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::var("DB_PATH").unwrap_or_else(|_| String::from("data/users.db")));
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let repo = Arc::new(SqliteUserRepository::open(&path).await?);
    let addr = std::env::var("HTTP_ADDR").unwrap_or_else(|_| String::from("127.0.0.1:8081"));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = api::app(Arc::clone(&repo));
    println!("database: {}", path.display());
    println!("Rust user API listening at http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    repo.close().await;
    Ok(())
}
