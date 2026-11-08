use crate::{cancel::CancelToken, errors::QueryError};
use std::{collections::HashMap, time::Duration};

#[derive(Clone)]
pub struct User {
    pub name: String,
}

pub trait UserRepository: Sync {
    fn find_by_id(&self, token: &CancelToken, id: i64) -> Result<Option<User>, QueryError>;
}

pub struct MemoryUserRepository {
    users: HashMap<i64, User>,
}

impl MemoryUserRepository {
    pub fn new() -> Self {
        Self {
            users: HashMap::from([
                (
                    1,
                    User {
                        name: String::from("Spring"),
                    },
                ),
                (
                    2,
                    User {
                        name: String::from("Lin"),
                    },
                ),
            ]),
        }
    }
}

impl UserRepository for MemoryUserRepository {
    fn find_by_id(&self, token: &CancelToken, id: i64) -> Result<Option<User>, QueryError> {
        token.check()?;
        let delay_ms = match id {
            1 => 240,
            2 => 80,
            404 => 160,
            500 => 120,
            _ => 50,
        };
        token.wait(Duration::from_millis(delay_ms))?;
        token.check()?;

        if id == 500 {
            return Err(QueryError::BackendUnavailable);
        }
        Ok(self.users.get(&id).cloned())
    }
}
