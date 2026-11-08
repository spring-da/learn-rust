# 第一课：从 Java 过渡到 Rust

这是 `E:\WorkProject\go\lesson01` 的同业务对照版本。
先完成 Go 的接收者实验，再用 RustRover 打开本目录。

```powershell
Set-Location E:\WorkProject\rust\lesson01
cargo run
```

程序输出如下；Cargo 还会显示编译、运行信息。

```text
after +10: name=Spring points=20
rejected -5: delta must be positive; points=20
owner moved: name=Spring points=20
```

## 按这个顺序读 src/main.rs

1. `struct User` 声明字段，`impl User` 声明关联到这个类型的方法。
2. `String::from("Spring")` 创建拥有字符串数据的 String；`i64` 对应 Java 的 long。
3. `let mut user` 声明可变绑定；没有 mut 时，不能这样修改它的字段。
4. `&mut self` 表示可变借用：方法临时获得修改权限，调用方仍保留所有权。
5. `Result<T, E>` 表达成功或失败；这里 T 是 `()`，表示成功时没有额外数据；E 是 `&'static str`，这里用来返回字符串字面量作为错误信息。生命周期细节后续再展开。
6. `Ok(())` 是成功值，`Err(...)` 是失败值。函数末尾没有分号的表达式作为返回值；所以这里没有写 return。
7. `match` 分别处理成功与失败；`println!` 的感叹号表示它是宏。

字段与方法默认不向外部模块公开；main 和 User 位于同一模块，所以本例可以直接访问。

## 原理：借用与移动

调用 `user.add_points(10)` 时，方法通过可变借用修改数据，调用完成后可以继续使用 user。
编译器会限制借用：当某份数据的可变借用仍在使用时，不能同时通过其他借用访问它。

`let moved_user = user` 则转移了本例 User 的所有权。User 含有 String，未实现 Copy，
赋值后原绑定 user 不能再作为完整的 User 使用；本例尝试读取它的 name 字段也会报错。
这既不是复制一个 Java 对象引用供两边继续使用，也不是深拷贝字符串。

普通整数等 Copy 类型的赋值会复制，所以“Rust 所有赋值都会移动”是错误的概括。

moved_user 离开作用域时，User 的字段会被清理，其中 String 持有的堆内存会被释放。
Rust 通过所有权、借用和析构来管理本例内存，不依赖追踪式 GC。
移动是语言语义，不等于每次都必须在机器层面搬运内存。

## 动手实验

1. 取消源文件最后那行 println! 的注释，然后运行 `cargo check`。读报错后重新注释这一行，使项目恢复可编译。
2. 去掉 `let mut user` 中的 mut，运行 `cargo check`，观察为何 add_points 无法获得可变借用。然后恢复 mut。
3. 实现 `spend_points(&mut self, delta: i64) -> Result<(), &'static str>`：delta 必须大于 0；余额不足返回错误；失败不修改积分。练习时放在移动所有权之前调用。

## 对应的官方说明

- [所有权、移动与 Copy](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html)
- [方法与 self 借用](https://doc.rust-lang.org/book/ch05-03-method-syntax.html)
- [Result 错误处理](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
