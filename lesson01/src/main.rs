struct User {
    name: String,
    points: i64,
}

impl User {
    // 可变借用：方法可以修改 User，调用方保留所有权。
    fn add_points(&mut self, delta: i64) -> Result<(), &'static str> {
        if delta <= 0 {
            return Err("delta must be positive");
        }
        self.points += delta;
        Ok(())
    }
}

fn main() {
    let mut user = User {
        name: String::from("Spring"),
        points: 10,
    };

    match user.add_points(10) {
        Ok(()) => println!("after +10: name={} points={}", user.name, user.points),
        Err(err) => {
            println!("unexpected error: {err}");
            return;
        }
    }

    match user.add_points(-5) {
        Ok(()) => println!("unexpected success"),
        Err(err) => println!("rejected -5: {err}; points={}", user.points),
    }

    // User 含有 String，且未实现 Copy；这里转移所有权。
    let moved_user = user;
    println!(
        "owner moved: name={} points={}",
        moved_user.name, moved_user.points
    );

    // 练习：取消下一行的注释，运行 cargo check，阅读编译器的报错。
    // println!("old owner: {}", user.name);
}
