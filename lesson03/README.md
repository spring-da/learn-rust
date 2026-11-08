# 第三课：文件仓库与 ? 错误传播

业务与 Go 版一致：从 JSON 文件读取用户，区分找到、没找到和查询失败。
目标：理解 Result<Option<T>, E>、?、From，以及返回拥有所有权的值。

## 运行与阅读顺序

用 RustRover 打开本目录，终端以本目录作为工作目录。

```powershell
Set-Location E:\WorkProject\rust\lesson03
cargo run
```

本课首次使用外部依赖。Cargo.toml 声明 serde 和 serde_json，Cargo 首次构建时会下载依赖。
serde 的 derive feature 启用用于生成反序列化实现的宏。
Cargo.lock 记录具体解析到的版本，应用项目应保留它。

若官方源连接慢，可以只为这次运行使用镜像；不修改全局 Cargo 配置。本例已通过此命令完成下载与运行验证：

```powershell
cargo --config 'source.crates-io.replace-with="rsproxy-sparse"' --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' run
```

镜像参数依据 [RsProxy 配置说明](https://rsproxy.cn/)。

先看 data/users.json，再看 src/repository.rs，接着看 src/errors.rs，最后看 src/main.rs。

程序输出的主要行是：

```text
case=found result=hello, Spring
case=absent result=user 404 not found
case=missing-file error=missing-file
  detail: ...
case=invalid-json error=invalid-json
  detail: ...
case=found-again result=hello, Lin
```

detail 含有路径、ID 和具体错误，文件系统消息可能随操作系统和语言变化。
data/missing.json 故意不存在；data/broken.json 的尾随逗号故意保留为语法错误。
这些都是演示输入，main 显式处理错误后继续下一次循环，不表示仓库函数在失败后继续执行。

## 三种结果

| 状态 | find_by_id 返回 | 含义 |
|---|---|---|
| 找到 | Ok(Some(user)) | 查询完成且存在用户 |
| 没找到 | Ok(None) | 查询完成但不存在用户 |
| 查询失败 | Err(error) | 文件读取或数据解析失败 |

外层 Result 表达操作成败，内层 Option 表达成功后是否存在用户。
这对应 Go 本课的 `(User, bool, error)`。

## ? 做了什么

```rust
let content = fs::read_to_string(&self.path)?;
```

在这里可以展开成：

```rust
let content = match fs::read_to_string(&self.path) {
    Ok(value) => value,
    Err(err) => return Err(RepositoryError::from(err)),
};
```

成功时取出值；失败时将错误转换成当前函数的错误类型并提前返回。
? 不会 panic，也不会自动打印、重试或忽略错误。
它同样用于 welcome 中，将仓库错误传递给 main。

为什么 io::Error 与 serde_json::Error 都能用 ?？
errors.rs 为 RepositoryError 实现了两种 From 转换，把它们分别装进 Read 和 Decode 变体。
From 是一个转换 trait，? 的错误传播会用到这些实现。
标准 Error trait 的 source 方法暴露底层原因，但 ? 的类型转换本身依赖 From，而不是 source。

## JSON 与所有权

`#[derive(Deserialize)]` 在编译期间生成 User 的反序列化 trait 实现，可以联系 Java 的代码生成机制理解。
`serde_json::from_str` 将文本转换成 `Vec<User>`；Vec 可以先类比一个拥有元素的可增长数组。
`fs::read_to_string` 自己处理文件的打开、读取和关闭，不需要这里手动关闭句柄。

上一课返回 Option<&User>，借用的是长期存放在仓库里的数据。
本课的 Vec 是函数内部的临时变量，函数返回后会被清理，不能返回指向它内部用户的引用。
因此改为 Option<User>：

```rust
Ok(users.into_iter().find(|user| user.id == id))
```

into_iter 消费 Vec，并按值迭代；find 取出第一个满足条件的 User，将其所有权交给调用方。
`|user| user.id == id` 是判断条件的闭包，可以联系 Java 的 Lambda 理解。
这里没有 clone 整个用户；其他未返回的元素会被清理。

## 模块与错误协议

`mod errors; mod repository;` 声明模块，编译器按模块规则加载对应的 .rs 文件。
`pub` 允许其他模块访问相关类型、方法和字段；`crate::errors` 从当前 crate 的根模块定位 errors。

errors.rs 可以先重点读 enum 和两个 From 实现。
Display 控制 `{err}` 的展示，Error::source 暴露底层原因，Debug 提供调试表示。
RepositoryError 保存原始错误值，所以 main 可以按 Read/Decode 变体及内部错误信息分类。

本例每次查询重新读取文件；没有缓存，也没有实现业务数据校验。
Serde 默认忽略未知结构体字段，但本例 User 缺少必需字段时会报错。
Go 的 json.Unmarshal 对缺失字段通常保留零值，两种库的默认规则不完全相同。

## 动手练习

1. 在本课 Rust 源码中找到三个 ?，说明每一个失败时从哪个函数提前返回。
2. 将错误类型转换 `RepositoryError::from(err)` 对照到 errors.rs 的具体 From 实现。
3. 暂时把 users.into_iter() 改成 users.iter()，运行 cargo check，观察 Option<User> 与 Option<&User> 的类型差异，再恢复。仅把返回类型改成引用也无法安全返回对局部 Vec 的借用。
4. 修复 data/broken.json，再运行，观察原 invalid-json 案例成功。实验后可恢复故障文件。

Go 对照项目：`E:\WorkProject\go\lesson03`。

## 官方说明

- [? 与错误传播](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
- [Serde derive](https://serde.rs/derive.html)
- [serde_json::from_str](https://docs.rs/serde_json/latest/serde_json/fn.from_str.html)
- [Error::source](https://doc.rust-lang.org/std/error/trait.Error.html#method.source)
