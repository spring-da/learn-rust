# 第七课：Axum HTTP API 与异步等待

目标：接收真实 HTTP 请求，通过路由与提取器取得参数和共享仓库，把业务结果转换为状态码和 JSON。

## 启动并请求

```powershell
Set-Location E:\WorkProject\rust\lesson07
cargo run
```

默认监听 `127.0.0.1:8081`，保持该终端运行，在另一个终端请求：

```powershell
curl.exe -i http://127.0.0.1:8081/users/1
curl.exe -i http://127.0.0.1:8081/users/abc
curl.exe -i http://127.0.0.1:8081/users/404
curl.exe -i http://127.0.0.1:8081/users/500
curl.exe -i http://127.0.0.1:8081/users/99
```

成功响应为 HTTP 200 和 `{"id":1,"name":"Spring"}`。使用 Ctrl+C 停止服务。
若端口占用，可先设置 `$env:HTTP_ADDR = '127.0.0.1:18081'`，再启动并调整请求端口。

首次构建下载 Axum、Tokio 等依赖；Cargo.lock 记录解析到的具体版本。
官方源下载慢时，可以只为本次命令使用镜像：

```powershell
cargo --config 'source.crates-io.replace-with="rsproxy-sparse"' --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' run
```

此命令不修改全局 Cargo 配置，参数依据 [RsProxy 配置说明](https://rsproxy.cn/)。

## 本例接口约定

| 请求或结果 | HTTP 状态码 | JSON 内容 |
|---|---|---|
| GET /users/1 或 /users/2 | 200 | 用户对象 |
| 非数字、非正数、超出 i64 的 ID | 400 | id must be a positive 64-bit integer |
| GET /users/404 | 404 | user not found |
| GET /users/500，模拟仓库故障 | 503 | repository unavailable |
| GET /users/99，模拟 600ms 慢查询 | 504 | query deadline exceeded |
| POST /users/1 | 405 | method not allowed，Allow 为 GET, HEAD |
| GET /missing | 404 | route not found |

错误体为 `{"error":"..."}`。普通查询等待 20ms，单次查询有 300ms 预算。
其他未存储的有效正整数通常返回 404。99 和 500 是特意安排的演示输入。

## 先联系 Spring MVC

| Java 中熟悉的概念 | 本例 Rust 写法 |
|---|---|
| @GetMapping("/users/{id}") | Router.route("/users/{id}", get(get_user)) |
| @PathVariable | Path(raw_id): Path<String> 加 parse 校验 |
| Controller 的仓库依赖 | State(state): State<AppState> |
| JSON 响应转换 | Json(user) 与 Serialize |
| 错误到 HTTP 响应的转换 | ApiError 实现 IntoResponse |

阅读顺序为 main.rs → api.rs 的 app/get_user → repository.rs → ApiError 的 IntoResponse。
测试模块可在完成上述阅读后再看。

## Tokio、async 与 await

`#[tokio::main]` 建立异步运行时，再运行 async main。
调用 async 函数得到 future；运行时轮询这些 future，遇到尚未完成的异步等待时可调度其他就绪任务。
await 是等待 future 完成的语法，只有等待尚未就绪时才让出执行机会。
它不会自动把任意阻塞代码变成异步，也不表示每个 async 函数拥有独立 OS 线程。

仓库使用 `tokio::time::sleep(...).await`，等待期间释放执行机会给运行时。
它与上一课的 `std::thread::sleep` 的执行方式不同。

## Arc 与 State

Router 持有 AppState，State 提取器将状态提供给处理器。
AppState 可 Clone，其中 Arc<MemoryUserRepository> 的克隆增加共享所有权计数，所有请求访问同一个仓库。
这里没有复制整个 HashMap。

仓库初始化后只读，Arc 负责所有权共享。若以后修改内部数据，仍需相应的同步设计。
Arc 本身不会为任意业务操作自动加锁。

## 超时与嵌套结果

```rust
tokio::time::timeout(QUERY_TIMEOUT, state.repo.find_by_id(id)).await
```

返回形状为 `Result<Result<Option<User>, 仓库错误>, Elapsed>`：

- Err(Elapsed)：查询时间预算用完，转换为 504。
- Ok(Err(error))：查询结束但仓库失败，记录日志并返回 503。
- Ok(Ok(None))：仓库正常但用户不存在，返回 404。
- Ok(Ok(Some(user)))：成功返回用户 JSON。

本例超时后丢弃被等待的查询 future，使其异步计时等待结束。
future 取消不保证撤销已经发生的外部副作用；处理真实数据库写入时还需事务与业务语义。
如果一个 future 执行长时间阻塞计算而不让出执行机会，timeout 也可能无法及时观察到期限。

## IntoResponse

处理器返回 `Result<Json<User>, ApiError>`，Axum 将两种分支转换为 HTTP 响应。
ApiError 实现 IntoResponse，集中规定状态码、JSON 错误体与 Allow 等响应头。
底层仓库错误保持在日志中，客户端得到本例约定的稳定错误文案。

## 验证与练习

```powershell
cargo test
```

下载慢时，在测试命令前使用与 run 相同的两组 --config 参数。
测试通过 Router 发送请求，验证状态、JSON、参数边界与方法约束。

练习：增加 GET /health，返回 HTTP 200 和 `{"status":"ok"}`，不访问仓库，并为它补一条接口检查。

Go 对照项目：`E:\WorkProject\go\lesson07`，默认端口为 8080。

## 官方说明

- [Axum 入门与响应模型](https://docs.rs/axum/latest/axum/)
- [State 提取器](https://docs.rs/axum/latest/axum/extract/struct.State.html)
- [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)
- [Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html)
