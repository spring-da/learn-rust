# 第 26 课：Axum HTTP 层与持久化调度

本课通过 path dependency 复用第 24 课 Store 与第 25 课 Runner，使用 Axum、Tokio、serde_json 与本地 HTTP 演示客户端。

```powershell
Set-Location E:\WorkProject\rust\lesson26
cargo test --locked
cargo run --locked -- --demo
```

首次需要下载依赖。--demo 自动使用临时端口、新建 data/http-demo-*.db，验证重试、幂等、冲突、取消、服务停止和同库重启，最后自行退出。

## 手动运行

```powershell
cargo run --locked -- --db 'E:\WorkProject\rust\lesson26\data\tasks.db' --addr '127.0.0.1:8081'
```

另开 PowerShell：

```powershell
$base = 'http://127.0.0.1:8081'
$body = @{kind='flaky'; payload='hello queue'; max_attempts=3; fail_until=2} | ConvertTo-Json
$task = Invoke-RestMethod -Method Post -Uri "$base/tasks" -Headers @{'Idempotency-Key'='manual-example'} -ContentType 'application/json; charset=utf-8' -Body $body
Invoke-RestMethod "$base/tasks/$($task.id)"
```

退出使用 Ctrl+C，或 `Invoke-RestMethod -Method Post "$base/demo/shutdown"`。

## 类型如何连接三层

- Spec 由 JSON 反序列化，必填 kind、payload、max_attempts，可选整数默认 0；拒绝未知字段。
- AppState 持有 Arc<Store>、可克隆的 Runner Control 与关闭通知。
- 同步 SQLite 调用通过 spawn_blocking 执行，Mutex<Connection> 不跨异步 await 持有。
- worker 使用标准线程；HTTP 使用异步运行时。两者通过持久化状态与控制信号衔接。

数据库查询返回拥有字段的 Task，再序列化成 JSON，避免把数据库游标或连接借用暴露到异步响应的生命周期中。

## 与 Go 相同的接口

| 请求 | 含义 |
| --- | --- |
| POST /tasks | 201 创建任务资源，包含 Location 与 Idempotency-Replayed |
| GET /tasks/{id} | 查询当前状态 |
| GET /tasks | 按 ID 返回最早 100 条 |
| POST /tasks/{id}/cancel | 202 接收取消请求 |
| GET /readyz | 200 就绪，关闭或后台失败时 503 |
| POST /demo/shutdown | 202 并启动关闭 |

Idempotency-Key 必须只有一个值，1～64 个 ASCII 字母、数字、-、_。
参数约束见第 24 课，请求体最多 4096 字节。成功重放返回同一个任务的当前状态。
JSON/键/ID 格式错误 400，不存在 404，冲突或重复取消 409，过大 413，媒体类型错误 415，参数范围错误 422，未完成容量满 429。

客户端请求结束后，任务可以继续运行。取消通过持久化标记影响结果提交；它不由 HTTP Future 是否仍存活决定。

## watch 与关闭顺序

watch 保存关闭状态，避免“通知发生在开始等待之前”造成信号丢失。
关闭 Future 将 ready 置为 false，停止 worker 继续领取，然后 Axum 等待连接与请求收尾。
主流程再 join worker，最后释放 Store 与 owner 文件锁。
[Axum graceful shutdown](https://docs.rs/axum/0.8.9/axum/serve/struct.Serve.html#method.with_graceful_shutdown)

未完成任务会留给下次启动恢复，已持久化取消会继续生效。Store 的恢复策略不会因为换成 HTTP 调用就改变。
同步数据库操作不能因为放进 async fn 就自动变成异步，因此显式隔离到阻塞任务池。

## 验证与练习

五个接口测试覆盖严格输入、幂等、16 个并发同键请求、取消/容量和关闭；--demo 覆盖真实 TCP、后台重试与同库重启。
另外实际验证过：Go 运行中被强制结束后，Rust 接续同一个任务并完成；两种程序也遵守同一 owner 锁和幂等数据。

1. 为数据库阻塞调用增加指标，区分等连接锁和执行 SQL 的耗时。
2. 为 GET /tasks 增加游标分页，并保留返回顺序。
3. 明确多个服务副本共享任务库所需的租约策略，不能直接去掉 owner 锁后继续执行当前启动恢复。

本批脚本和学习路线位于 ../lesson24，三个目录应一起保留。
