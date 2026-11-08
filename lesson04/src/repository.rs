use std::{collections::HashMap, thread, time::Duration};

#[derive(Clone)]
pub struct User {
    pub name: String,
}

// Sync 表示对仓库的共享引用可以安全地跨线程使用。
pub trait UserRepository: Sync {
    fn find_by_id(&self, id: i64) -> Result<Option<User>, &'static str>;
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
    fn find_by_id(&self, id: i64) -> Result<Option<User>, &'static str> {
        let delay_ms = match id {
            1 => 240,
            2 => 80,
            404 => 160,
            500 => 120,
            _ => 50,
        };
        thread::sleep(Duration::from_millis(delay_ms));

        if id == 500 {
            return Err("simulated backend unavailable");
        }
        // 复制匹配的 User，结果拥有自己的 String。
        Ok(self.users.get(&id).cloned())
    }
}
