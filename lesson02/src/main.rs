use std::collections::HashMap;

struct User {
    name: String,
}

trait UserRepository {
    fn find_by_id(&self, id: i64) -> Option<&User>;
}

struct MemoryUserRepository {
    users: HashMap<i64, User>,
}

// 明确声明：为 MemoryUserRepository 实现 UserRepository。
impl UserRepository for MemoryUserRepository {
    fn find_by_id(&self, id: i64) -> Option<&User> {
        self.users.get(&id)
    }
}

// 借用 trait 对象，通过它调用具体仓库实现。
fn welcome(repo: &dyn UserRepository, id: i64) -> String {
    match repo.find_by_id(id) {
        Some(user) => format!("hello, {}", user.name),
        None => format!("user {id} not found"),
    }
}

fn main() {
    let repo = MemoryUserRepository {
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
    };

    for id in [1, 2, 404] {
        println!("id={id} -> {}", welcome(&repo, id));
    }

    // 练习：增加 EmptyUserRepository，再把它传给 welcome。
}
