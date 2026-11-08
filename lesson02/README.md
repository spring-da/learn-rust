# 第二课：用 trait 替换用户仓库

业务与 Go 版一致：根据 ID 查用户，再生成欢迎语或未找到提示。
目标：理解显式 trait 实现、Option、借用返回值和 trait 对象。

## 运行

用 RustRover 打开本目录，或者在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson02
cargo run
```

程序输出如下，Cargo 还会显示编译和运行信息：

```text
id=1 -> hello, Spring
id=2 -> hello, Lin
id=404 -> user 404 not found
```

## 按这个顺序阅读

1. `use std::collections::HashMap`：引入标准库的哈希表。
2. `trait UserRepository`：声明“根据 ID 查用户”的行为。
3. `impl UserRepository for MemoryUserRepository`：明确为具体类型实现这个 trait。
4. `self.users.get(&id)`：查询时借用键；返回 `Option<&User>`。
5. `welcome`：处理找到和没找到两个分支。
6. `main`：使用 `HashMap::from` 构造仓库，再把仓库借给业务函数。

`format!` 返回新构造的 String，`println!` 把内容打印出来。
`get(&id)` 这一行没有分号，它的表达式值就是 find_by_id 的返回值。

## Option 与 Java Optional

可以先将 `Option<T>` 类比成 Java 的 `Optional<T>`，两者都表达“有值或没有值”。
Rust 的 Option 是枚举：

```rust
enum Option<T> {
    Some(T),
    None,
}
```

Some(user) 表示找到，None 表示没找到。通过 match 可以同时处理两种情况，
不需要先取一个可能为空的 User 再访问它的字段。
试着暂时移除 None 分支，运行 `cargo check`，观察编译器的穷尽性检查，然后恢复。

不要和第一课的 Result 混淆：Option 表达有无；Result 表达成功或错误。
本例内存仓库没有模拟存储故障，因而查询只返回 Option。
将来若需要区分“找到、没找到、查询失败”，可以使用 `Result<Option<User>, E>` 等类型。
其中 User 用拥有所有权的值还是引用，取决于具体的数据来源和 API 设计。

## 为什么是 Option<&User>

HashMap 拥有 User；查询只借出只读引用，没有复制 String，也没有把用户从仓库中移走。
因此 welcome(&repo, 1) 可以重复调用。
与 Go 版返回一个 User 值不同，本例 Rust 返回的是指向仓库中 User 的借用。

引用的有效期受到仓库借用的约束，不能在仓库销毁后继续使用。
这个方法省略了显式生命周期标注，编译器按规则将返回引用的生命周期关联到 &self。
当前只需要理解这个约束，生命周期语法以后再写。

## &dyn UserRepository 是什么

`&` 表示借用；`dyn UserRepository` 表示通过 trait 对象使用这个行为接口。
本例采用动态分派：运行时通过 trait 对象的方法表调用具体实现的方法。
可以把它联系到 Java 的接口调用和 Go 的接口调用来理解。

Rust 也可以通过泛型使用 trait，并不总是动态分派；本课先采用与 Java 接口调用接近的形式。

## 动手练习

1. 新增 `struct EmptyUserRepository;`，为它实现 UserRepository，find_by_id 始终返回 None。
2. 在 main 中把 `&EmptyUserRepository` 传给 welcome，保留原内存仓库调用，观察同一函数使用两种实现。
3. 在内存仓库中加入 ID 为 3、name 为 `String::new()` 的用户，再查询 3 和 404。

完成标准：welcome 的代码保持不变，两种仓库都能使用；空名字用户为 Some，不存在用户为 None。

Go 对照项目：`E:\WorkProject\go\lesson02`。

## 官方说明

- [定义与实现 trait](https://doc.rust-lang.org/book/ch10-02-traits.html)
- [Option 枚举](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html#the-option-enum-and-its-advantages-over-null-values)
- [HashMap::get 返回借用](https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.get)
- [trait 对象与动态分派](https://doc.rust-lang.org/book/ch18-02-trait-objects.html)
