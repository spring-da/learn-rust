# 第十二课：Outbox 与后台通知任务

目标：注册成功时可靠记录通知任务，后台投递失败后重试，重启后继续处理，并让接收端安全处理重复事件。

## 先运行案例

停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson12
$env:DB_PATH = 'E:\WorkProject\rust\lesson12\data\outbox.db'
$env:NOTIFY_DB_PATH = 'E:\WorkProject\rust\lesson12\data\notifications.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
$env:WORKER_ENABLED = '1'
$env:DEMO_FAIL_FIRST = '1'
$env:DEMO_LOSE_ACK_ONCE = '0'
cargo run --locked
```

首次使用两个数据库时为空，重启保留数据。本课不会迁移第十一课的旧数据库。
默认每秒轮询一次，每轮最多投递一个到期事件；reqwest HTTP 超时为 2 秒。
本地示例使用 HTTP，reqwest 关闭了默认功能且只开启 json 功能。
如果曾设置 NOTIFY_URL，请清除该变量，以使用当前进程内置的接收端。

另开一个 PowerShell，注册用户：

```powershell
'{"name":"Alice","initial_points":100}' | Set-Content -Encoding ascii request.json
curl.exe -i http://127.0.0.1:8081/users -H 'Content-Type: application/json' -H 'Idempotency-Key: reg-001' --data-binary '@request.json'
```

201 表示用户、账户、请求幂等记录、待投递事件已提交；通知由后台任务随后投递。
DEMO_FAIL_FIRST=1 让接收端在本次进程的第一条合法通知处理前返回 503。
观察日志：delivery failed、retry scheduled，随后出现 delivery confirmed。
重复相同注册请求只重放用户结果，不增加事件。

查看发送端和接收端：

```powershell
curl.exe -s http://127.0.0.1:8081/outbox
curl.exe -s http://127.0.0.1:8081/notifications
```

全新数据库中，最终应看到一个 status=sent、attempts=2 的事件，以及一条通知。
两次查询返回数组，各最多显示最早的 100 条记录。注册 API 的输入规则与上一课一致。

## 把“需要通知”与注册一起保存

```text
注册事务：users + points_accounts + idempotency_records + outbox_events
                                      ↓ 提交
后台任务：读到期事件 → HTTP POST /notifications → 更新 Outbox 状态
                                      ↓
接收端独立数据库：notifications（event_id 唯一）
```

outbox.db 保存发送端业务数据与事件，notifications.db 保存接收端已处理的通知。
两端为了方便演示由同一程序提供路由，投递经过真实 HTTP。
NOTIFY_URL 可以指向另一案例进程的 /notifications；接收记录保存在那一端的 NOTIFY_DB_PATH。

本例以 notifications 表的一条记录表示“通知已处理”，不发送真实邮件。
去重与这个处理效果在接收端同一个事务内完成。
若以后接入真实邮件提供方，还需要解决邮件调用与本地记录之间的故障边界。

按 src/repository.rs 的 create 阅读：

```text
BEGIN IMMEDIATE
  查询请求幂等键；已有成功结果则重放并结束
  INSERT users
  INSERT points_accounts
  INSERT idempotency_records
  INSERT outbox_events
COMMIT
```

第四次写入失败也会撤销前三次写入；成功提交则一定留下待处理事件。
事件保存注册时的 user_id、name、points 快照。
event_id 由 SQLite 随机生成并持久保存；重发一直使用它，每个注册用户对应一个事件。
可以联系 Java 的 @Transactional：业务与 Outbox 在同一事务内保存，后台发送独立执行。

## 后台任务的所有权与退出

src/main.rs 把仓库交给两个使用者：API 与 worker。
仓库本身不复制，Arc::clone 增加共享引用；async move 把这份 Arc 和 sender 的所有权移进任务。

```rust
let worker_repo = Arc::clone(&repo);
let worker = tokio::spawn(async move {
    outbox::run_worker(worker_repo, sender, shutdown, interval, retry_delay).await;
});
```

tokio::spawn 要求 Future 为 Send + 'static。
Send 允许运行时在线程间调度任务；'static 在这里要求任务不借用可能提前结束的栈变量，
不是要求任务永远运行。拥有 Arc、String 等值可以满足这种所有权要求。

src/outbox.rs 用 interval 驱动轮询，用 watch 接收退出状态。
watch 保存最新状态，适合表达“现在应当停止”，不用于保存业务事件。
外层 select 等待定时器或退出；内层 select 允许在等待 HTTP 时取消当前投递。
取消可能发生在接收端已经处理之后，因此持久化去重仍然必要。

main 收到 Ctrl+C 后发送退出状态，等待 worker 的 JoinHandle，再关闭连接池。
只丢弃 JoinHandle 并不能据此认为后台任务已经结束。
Go 对照实现用 context、select 和完成通道表达取消与等待。

## 重试状态保存在哪里

dispatch_once 先查询 pending 且到期的事件，查询结束后才发 HTTP，不持有数据库事务等待网络。
2xx 标记 sent；超时或非 2xx 则保留 pending 并记录错误。
下次投递时间设为本次轮询时间加 2 秒，next_attempt_at 为 Unix 毫秒。
时间与状态保存在数据库中，所以重启不会丢掉尚未完成的任务。

attempts 是已完成并成功记录状态的投递尝试数。
发送后、更新状态前退出，可能来不及累加，所以它不是精确的 HTTP 调用总数。

## 接收端为什么也要幂等

```text
接收端已提交通知 → 成功确认丢失或标记 sent 失败 → 再次投递相同 event_id
```

src/notifications.rs 在事务中执行 INSERT ... ON CONFLICT(event_id) DO NOTHING。
同 ID、同内容返回 200，接收表不会新增一行；同 ID、不同内容返回 409。
持久化的 event_id 唯一约束让接收端重启后也能识别重复。

进阶演示：停止服务，设 DEMO_FAIL_FIRST=0、DEMO_LOSE_ACK_ONCE=1，重新启动并注册新名字、新请求键。
接收端先保存通知，再故意返回一次 503，模拟发送端无法获得成功确认。
重试最终得到 200，发送端标记 sent，接收端仍只有一条通知。
两个故障开关都是每次进程启动后各最多触发一次。

## 重启恢复演示

设 WORKER_ENABLED=0 后启动服务，注册一个新名字、新请求键。
新事件会停留在 pending，接收端没有对应通知。
按 Ctrl+C，保留两个数据库路径，把 WORKER_ENABLED 改为 1，再启动。
事件使用原 event_id 继续投递。恢复的是持久化待办，而不是旧进程的 Tokio task。

## 测试与阅读顺序

```powershell
Set-Location E:\WorkProject\rust\lesson12
cargo test --locked
```

保留上一课 9 项测试并扩展为检查四张发送端表；新增 6 项测试覆盖：
Outbox 写入失败的整体回滚、到期重试、双方数据库重开后的重复确认、标记 sent 失败后的安全重发、
16 个重复事件并发投递，以及等待 HTTP 时取消后台任务。
测试通过传入轮询时间验证重试边界，避免为了两秒间隔真的等待两秒。

推荐顺序：schema.sql → src/repository.rs → src/outbox.rs → src/notifications.rs → src/main.rs。
练习：在 mark_sent_failure_allows_safe_redelivery 中观察两个数据库，
说明为什么“发送成功”仍不能等同于“发送端已可靠记录完成”。

## 本例的范围

每个发送端数据库运行一个 worker，两种语言默认各使用独立数据库。
本课未实现多 worker 的领取与租约，接收端去重也不能替代任务调度。
失败持续重试，没有死信队列；不同事件的重试不承诺全局顺序。
生产系统需要根据业务设计退避、永久错误处理、保留策略和认证。

参考：[事务发件箱模式](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html)、
[Tokio spawn](https://docs.rs/tokio/latest/tokio/task/fn.spawn.html)、
[Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown)。
