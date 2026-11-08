use serde::Serialize;
use std::{collections::HashMap, sync::RwLock};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryError {
    NameExists,
    Unavailable,
}

struct Store {
    users: HashMap<i64, User>,
    names: HashMap<String, i64>,
    next_id: i64,
}

pub struct MemoryUserRepository {
    store: RwLock<Store>,
}

impl MemoryUserRepository {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(Store {
                users: HashMap::from([
                    (1, User { id: 1, name: String::from("Spring") }),
                    (2, User { id: 2, name: String::from("Lin") }),
                ]),
                names: HashMap::from([(String::from("Spring"), 1), (String::from("Lin"), 2)]),
                next_id: 3,
            }),
        }
    }

    pub fn find_by_id(&self, id: i64) -> Result<Option<User>, RepositoryError> {
        let store = self.store.read().map_err(|_| RepositoryError::Unavailable)?;
        Ok(store.users.get(&id).cloned())
    }

    pub fn create(&self, name: String) -> Result<User, RepositoryError> {
        let mut store = self.store.write().map_err(|_| RepositoryError::Unavailable)?;
        // 唯一性检查、ID 分配与两个索引的更新由同一个写锁保护。
        if store.names.contains_key(&name) {
            return Err(RepositoryError::NameExists);
        }
        let user = User { id: store.next_id, name };
        store.next_id += 1;
        store.names.insert(user.name.clone(), user.id);
        store.users.insert(user.id, user.clone());
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, sync::Barrier, thread};

    #[test]
    fn concurrent_name_uniqueness() {
        let repo = MemoryUserRepository::new();
        let start = Barrier::new(32);
        let outcomes = thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..32 {
                let repo = &repo;
                let start = &start;
                handles.push(scope.spawn(move || {
                    start.wait();
                    repo.create(String::from("Concurrent"))
                }));
            }
            handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|outcome| matches!(outcome, Err(RepositoryError::NameExists))).count(), 31);
    }

    #[test]
    fn concurrent_ids_and_reads() {
        let repo = MemoryUserRepository::new();
        let users = thread::scope(|scope| {
            let mut handles = Vec::new();
            for index in 0..24 {
                let repo = &repo;
                handles.push(scope.spawn(move || {
                    let user = repo.create(format!("user-{index}")).unwrap();
                    assert_eq!(repo.find_by_id(user.id).unwrap(), Some(user.clone()));
                    user
                }));
            }
            handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
        });
        let ids = users.iter().map(|user| user.id).collect::<HashSet<_>>();
        assert_eq!(ids.len(), 24);
        assert!(ids.iter().all(|&id| id >= 3));
    }
}
