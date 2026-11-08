use serde::Serialize;
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
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
                        id: 1,
                        name: String::from("Spring"),
                    },
                ),
                (
                    2,
                    User {
                        id: 2,
                        name: String::from("Lin"),
                    },
                ),
            ]),
        }
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>, &'static str> {
        let delay_ms = if id == 99 { 600 } else { 20 };
        // 异步等待期间把执行机会交给 Tokio 运行时。
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;

        if id == 500 {
            return Err("simulated storage connection failure");
        }
        Ok(self.users.get(&id).cloned())
    }
}
