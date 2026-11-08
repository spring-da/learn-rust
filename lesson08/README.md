# 第八课：创建用户与共享状态写入

目标：使用 Json 请求提取器和 Deserialize 接收 POST，进行字段校验，并用 RwLock 保护唯一性与 ID 分配。

## 启动并调用

```powershell
Set-Location E:\WorkProject\rust\lesson08
cargo run
```

默认地址为 `127.0.0.1:8081`。另开 PowerShell 创建并查询用户：

```powershell
$response = Invoke-WebRequest -Uri 'http://127.0.0.1:8081/users' -Method Post -ContentType 'application/json' -Body '{"name":"  Alice  "}'
$response.StatusCode
$response.Headers.Location
$response.Content
curl.exe -i http://127.0.0.1:8081/users/3
```

新启动的仓库预置 Spring/1、Lin/2。第一次创建应得到 201、Location `/users/3` 和 `{"id":3,"name":"Alice"}`。
重复创建 Alice 返回 409。要直接观察重复请求的 HTTP 响应，可以使用：

```powershell
'{"name":"Alice"}' | Set-Content -Encoding ascii create-user.json
curl.exe -i http://127.0.0.1:8081/users -H 'Content-Type: application/json' --data-binary '@create-user.json'
```

示例文件中的名字是 ASCII；其他语言的名字应通过 UTF-8 JSON 发送。
Ctrl+C 停止服务。端口占用时可先设置 `$env:HTTP_ADDR = '127.0.0.1:18081'`。
数据只在当前进程内存中，重新启动后恢复为预置数据。

依赖沿用第七课。下载慢时可只为本次命令使用镜像：

```powershell
cargo --config 'source.crates-io.replace-with="rsproxy-sparse"' --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' run
```

镜像参数依据 [RsProxy 说明](https://rsproxy.cn/)，不修改全局配置。

## 接口约定

| 情况 | 状态 | 响应 |
|---|---|---|
| POST /users，创建成功 | 201 | 用户 JSON 和 Location |
| 处理后的名字已经存在 | 409 | name already exists |
| JSON 语法、结构、类型或未知字段错误 | 400 | invalid JSON body |
| name 缺失、null、空白或长度非法 | 422 | name must contain 2 to 40 characters |
| 未提供 JSON Content-Type | 415 | Content-Type must be application/json |
| 请求体超过 1024 字节 | 413 | request body too large |
| GET /users/{id} | 200 / 400 / 404 | 用户、参数错误或不存在 |

名字去除首尾空白后需包含 2～40 个 Unicode 码点；按处理后的字符串判断重名，大小写敏感。
chars().count() 与 Go 的 RuneCountInString 在这里按码点计数，不等同于字节数或视觉字符簇。
本课仓库直接执行内存查找；未存储的有效 ID 返回 404。

## JSON 提取、解析与校验

CreateUserRequest 是请求 DTO，只包含 name。服务端决定新用户的 id。
Deserialize 由 derive 生成；deny_unknown_fields 拒绝额外字段；Option<String> 允许在字段校验阶段处理缺失或 null。
payload 使用 Result<Json<serde_json::Value>, JsonRejection>，这样处理器可以将提取失败转换为约定的 JSON 错误响应。
先确认根值为 JSON 对象，再使用 from_value 按 CreateUserRequest 解析字段，明确排除数组等输入形式。
DefaultBodyLimit::max(1024) 配置 Json 提取时的大小限制。

成功提取后，再去除首尾空白并校验长度；这一步失败返回 422。
请求提取涉及读取 body，因此位于处理器参数列表最后；State 在它之前提取。
底层 JSON 解析器对字段大小写、重复键等边界输入有各自规则，本例没有额外实现统一的跨语言解析器。

## Arc 和 RwLock 分别负责什么

Arc 让多个请求共同持有同一个 MemoryUserRepository。
仓库内部的 RwLock<Store> 管理读取与修改：多个读 guard 可以共存，写 guard 独占访问数据。
Store 把 users、names 和 next_id 放到同一把锁下，定义一次创建操作的同步边界。

```rust
let mut store = self.store.write().map_err(|_| RepositoryError::Unavailable)?;
if store.names.contains_key(&name) {
    return Err(RepositoryError::NameExists);
}
// 接着分配 ID 并更新两个索引；guard 在方法结束时释放。
```

先查重后释放锁、再获取锁写入，会允许两个请求同时通过检查。
本例从查重到保存都持有同一个写 guard，保证一个名字只能创建一次。
错误分支提前返回也会释放 guard。

仓库方法只做短暂的内存操作，使用标准库 RwLock，guard 不跨越 await。
长 I/O 或复杂计算应重新安排锁作用域和执行方式；异步函数中的同步锁等待依然会阻塞当前线程。
这次写入约束仅覆盖当前进程的共享仓库。以后落库，应通过数据库唯一约束和事务表达相应保证。

## 错误到 HTTP 的转换

RepositoryError::NameExists 通过 From 转为 ApiError::NameConflict，再由 IntoResponse 生成 409。
这让仓库处理数据约束，HTTP 层定义协议映射。
201 的 Location 指向新建资源，响应体给出实际保存的用户。
重名和无效请求都不会消耗 ID。

## 阅读和验证

先读 api.rs 的 create_user，再读 repository.rs 的 create，最后看 api_tests.rs 与仓库测试。

```powershell
cargo test
```

需要镜像时，在 test 前使用与 run 相同的两组 --config 参数。
检查覆盖创建后读取、空白处理、冲突、JSON 与字段边界、32 个同名并发请求只创建一次，以及并发 ID 与读取一致性。

练习：给请求增加可选 display_name，而唯一性仍只由 name 决定。先写清请求、响应与冲突规则，再修改 DTO 和仓库。

Go 对照项目：`E:\WorkProject\go\lesson08`，默认端口 8080。

## 官方说明

- [Serde 容器属性](https://serde.rs/container-attrs.html)
- [DefaultBodyLimit](https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html)
- [RwLock](https://doc.rust-lang.org/std/sync/struct.RwLock.html)
