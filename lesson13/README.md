# 第十三课：HTTP 服务的优雅关闭与请求取消

目标：停止接收新工作，为已有请求保留宽限期，超时再取消请求 Future，等待 handler 与 worker 结束后关闭数据库。

## 正常关闭实验

停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson13
$env:DB_PATH = 'E:\WorkProject\rust\lesson13\data\shutdown.db'
$env:NOTIFY_DB_PATH = 'E:\WorkProject\rust\lesson13\data\notifications.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
$env:SHUTDOWN_GRACE_MS = '5000'
$env:WORKER_ENABLED = '0'
cargo run --locked
```

先暂停 worker 以清楚观察 HTTP 请求，注册仍然会保存待投递事件。
这两个数据库独立于上一课，首次为空，重启保留数据。

另开 PowerShell：

```powershell
Set-Location E:\WorkProject\rust\lesson13
.\demo.ps1
```

脚本为每次实验生成新名字、新请求键，发起延迟 2 秒的注册，
确认服务正在处理后，再向 /demo/shutdown 发送关闭指令。
这个入口与 Ctrl+C 使用相同的关闭控制器，便于在 Windows 上稳定复现。

预期 SHUTDOWN_STATUS=202、REGISTRATION_STATUS=201；
服务输出 shutdown complete: forced=false active=0，然后进程退出。
开始退出并不会立即取消正在完成的注册。

## 超过宽限期的实验

在启动终端修改并重新启动：

```powershell
$env:SHUTDOWN_GRACE_MS = '500'
cargo run --locked
```

另一个终端重新运行 demo.ps1。2 秒的请求超过 500 毫秒宽限期，
服务会输出 grace ended，客户端收到 503，最后日志为 forced=true active=0。
在强制关闭的一般情形下，也要允许客户端遇到连接错误，不能依赖总能得到完整 HTTP 响应。

延迟发生在业务 handler 与 SQL 之前，因此这个被取消的演示请求不写四张业务表。
第一次正常注册仍然持久存在。
如果真实业务在取消之前已经提交，取消不会撤销提交；结果不确定时仍然需要幂等请求键。

## 两种信号不能混在一起

```text
running
  → stop：进入 draining、拒绝新工作、停止 listener、通知 worker 退出
  → 等待已有 HTTP 请求
      ├─ 宽限期内完成 → 正常结束
      └─ 宽限期耗尽 → force，取消各请求正在等待的 Future
  → 等待请求清理与 worker 结束
  → 关闭数据库连接池
```

SHUTDOWN_GRACE_MS 支持 1–10000，默认 5000，控制 HTTP 宽限期。
它不是进程的硬退出上限，连接归还与资源清理还要完成。
本课处理逻辑都在异步 await 点协作；阻塞线程或长时间不让出执行权的 CPU 循环需要另外设计停止方式。

## Rust 实现的三个关键点

先看 src/lifecycle.rs 的 serve_until_shutdown。
Axum 的 with_graceful_shutdown 负责停止接收并等待已接收请求，但本身没有本课需要的宽限期配置。
外层用 timeout(grace, &mut server) 等待；使用 &mut JoinHandle 保留任务句柄，超时后仍能继续协调与等待。

超时后，没有直接 abort 顶层 serve 任务就认为请求全部消失。
请求处理发生在各连接任务中，本课通过请求中间件传入 force 状态来取消实际处理逻辑。

```rust
tokio::select! {
    biased;
    _ = wait_true(&mut force) => cancelled_response(),
    response = async {
        // 演示延迟，然后执行实际业务 handler。
        next.run(request).await
    } => response,
}
```

这个片段展示结构，实际代码使用返回 503 的 error 函数。
force 分支完成时，另一个分支的 Future 被 drop，不再被当前中间件轮询。
这不会撤销已经完成的 SQL 提交，也不会自动取消业务自行 spawn 出去的独立任务；本课业务没有创建这种子任务。

第二个重点是 ActiveRequest：接收请求时增加计数，Drop 中减少计数。
正常返回、提前返回、Future 被取消都会走这个清理路径，类似 Go 的 defer leave()。
短时间使用 std::sync::Mutex 更新状态，锁不会跨 await 持有。
是否允许请求与进入 draining 使用同一把锁，避免关闭开始时又接收新业务。

第三个重点是 watch：stop、force、state_changed 分别表达关闭开始、强制取消、计数变化。
wait_true 先检查当前值再等待变化，避免在订阅时信号已经为 true 却漏掉退出。
wait_drained 等到 draining=true 且 active_requests=0，然后才允许外层关闭连接池。

main.rs 最后等待 worker 的 JoinHandle，再关闭两个池。
SQLx 事务被取消时需要通过 Drop 安排回滚与连接清理，pool.close().await 也会等待连接归还。
可以联系 Java 的停止接收任务、等待任务完成、最后关闭连接池的顺序理解；
Rust 通过所有权、Future 的 Drop 与显式 await 表达这些边界。

## 演示入口

| 接口或参数 | 含义 |
| --- | --- |
| GET /readyz | 正常时 200，draining 状态不就绪 |
| GET /demo/state | draining 与 active_requests |
| POST /demo/shutdown | 返回 202，触发与 Ctrl+C 相同的关闭流程 |
| X-Demo-Delay-Ms | 在业务 handler 之前等待 0–10000 毫秒，最多一个值 |
| 注册、用户查询、Outbox 与通知路由 | 沿用上一课 |

demo/state 与 demo/shutdown 不计入业务请求数，避免查询状态影响实验。
关闭 listener 后新连接可能失败，不能依赖此时还能从同一端口读到状态。
示例默认使用本机回环地址，demo 入口用于课程实验。

## 保留 Outbox 的未完成工作

正常关闭后保留数据库路径，将 WORKER_ENABLED 设为 1、SHUTDOWN_GRACE_MS 改回 5000，再启动。
注册保存的 pending 事件可以继续发送。
退出不要求把所有 pending 事件发送完；它们已持久化，可以交给下一次进程处理。
通知调用被取消时，如果接收端已经完成处理，仍然通过 event_id 去重避免重复效果。

## 验证与练习

```powershell
Set-Location E:\WorkProject\rust\lesson13
cargo test --locked
```

保留前 15 项测试，新加 5 项：draining 拒绝新工作和 readiness、宽限期内注册成功并持久化、
超时取消延迟注册、超时取消未发送完整的请求体，以及非法演示延迟不进入业务处理。
测试通过本机 HTTP 执行关键路径，直接查询四张物理表验证结果。

阅读顺序：serve_until_shutdown → protect → ActiveRequest → main.rs → lifecycle_tests.rs。
练习：说明为什么发出 stop 后不立刻发送 force；
再说明丢弃一个 JoinHandle 为什么不等于已经等待那个任务结束。

参考：[Axum with_graceful_shutdown](https://docs.rs/axum/latest/axum/serve/struct.Serve.html#method.with_graceful_shutdown)、
[Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown)、
[Tokio JoinHandle](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)。
