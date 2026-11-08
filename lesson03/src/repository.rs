use crate::errors::RepositoryError;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
}

pub trait UserRepository {
    fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError>;
}

pub struct FileUserRepository {
    pub path: String,
}

impl UserRepository for FileUserRepository {
    fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError> {
        let content = fs::read_to_string(&self.path)?;
        let users: Vec<User> = serde_json::from_str(&content)?;

        // 消费临时 Vec，把匹配 User 的所有权交给调用方。
        Ok(users.into_iter().find(|user| user.id == id))
    }
}

pub fn welcome(repo: &dyn UserRepository, id: i64) -> Result<String, RepositoryError> {
    let user = repo.find_by_id(id)?;
    Ok(match user {
        Some(user) => format!("hello, {}", user.name),
        None => format!("user {id} not found"),
    })
}
