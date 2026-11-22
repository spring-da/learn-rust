# 第 23 课：综合项目——有界任务池、取消、重试与结果导出

本课整合 module、trait object、FnMut、Send、所有权和同步状态。全部使用标准库。

```powershell
Set-Location E:\WorkProject\rust\lesson23
cargo run --offline
cargo test --offline
cargo run --offline -- --read data/results.tsv
```

`--out 路径` 修改输出位置，默认覆盖本课 data/results.tsv。
src/pool.rs 管理状态与 worker，src/records.rs 编解码，src/main.rs 组装四个案例；tests 中有 9 个测试。

| ID | 行为 | 终态 | 尝试次数 |
| --- | --- | --- | --- |
| 1 | 立即成功 | succeeded | 1 |
| 2 | 前两次失败，第三次成功 | succeeded | 3 |
| 3 | 预算两次，始终失败 | failed | 2 |
| 4 | 等待取消 | cancelled | 0 或 1 |

取消可能在任务排队或已经运行时发生，所以第 4 项的尝试次数允许不同。

## 读懂任务回调的类型

```rust
Box<dyn FnMut(&Cancellation, usize) -> Result<(), String> + Send + 'static>
```

| 部分 | 为什么需要 |
| --- | --- |
| Box | 队列拥有任务闭包 |
| dyn FnMut | 保存不同类型闭包，允许同一个闭包多次尝试、修改自己的捕获状态 |
| &Cancellation | 每次尝试借用取消信号 |
| usize | 当前尝试次数，从 1 开始 |
| Send | 任务会从提交线程移动到 worker 线程 |
| 'static | 捕获内容不能依赖提交栈中短期借用；不表示任务要永远运行 |

Submit 按值接收闭包并装箱。工作队列拥有它，worker 收到后独占调用。
票据 Ticket 则通过 Arc 共享状态，Mutex 保护状态更新，Condvar 唤醒等待者。
[thread::spawn](https://doc.rust-lang.org/std/thread/fn.spawn.html) 的 Send 与 'static 约束解释了跨线程的要求。

## 有界队列与 worker

sync_channel 限制等待任务容量，try_send 在满时立即返回 QueueFull。
workers 决定同时执行任务的最大数量。一个工作线程只在接收任务时持有 receiver 的 Mutex，执行回调前会释放它。
如果把锁保留到回调执行之后，其他 worker 将无法正常并行领取任务。
[sync_channel](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html) 提供有界缓冲与单接收端，本课在其上组织多个 worker。

Rejected 提交可能已经消耗 ID；ID 用于标识，不承诺连续。

## 状态、取消与重试

queued 进入 running，每次尝试前更新次数。成功变 succeeded，预算耗尽或任务 panic 变 failed。
错误在预算内立即重试，同一个 worker 继续处理；本课没有退避与独立延迟队列。

Ticket.cancel 与结果提交使用同一状态锁。取消先登记时，worker 以 cancelled 提交；终态先提交时，后续 cancel 返回 false。
排队取消由 worker 领取时确认，跳过回调；运行中回调通过 Cancellation.is_cancelled 检查请求。
wait 超时不会自动取消任务，终态快照的修改不会改变共享状态。

close 丢弃队列发送端，排空已接受工作并 join 所有 worker，可以重复调用；Pool 的 Drop 也会收尾。
正常关闭依赖回调协作结束。取消不会撤销已发生的副作用，重试的外部操作需要幂等。
本课默认 unwind 配置下会捕获任务 panic 并继续处理后续任务；panic=abort 时不适用。
[catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html) 只捕获展开式 panic；普通业务失败继续用 Result 表达。

## 终态结果持久化的范围

文件包含版本头和每条任务的 ID、终态、尝试次数、错误信息。错误字符串的 UTF-8 字节用十六进制编码，支持中文、制表符和换行。
Go/Rust 使用相同协议，读取验证版本、ID 唯一性、终态、次数与编码。

**这是一份终态结果报表，不是可崩溃恢复的任务队列。** 按指定路径覆盖写入，没有事务式原子提交。
重新读取能够恢复结果查询，运行中的任务及闭包没有被序列化。
若要恢复执行，需要持久化任务类型与参数，通过执行器注册表重建任务，并处理重复执行；可以接回 SQLite 与幂等课程。

读取 Go 的结果：

```powershell
cargo run --offline -- --read 'E:\WorkProject\go\lesson23\data\results.tsv'
```

## 测试观察什么

7 个任务池测试覆盖容量、最大并发、重试、两种取消、终态不可变、panic 隔离、关闭排空与等待超时。
测试使用 gate 控制回调继续执行，并用 Drop 守卫确保断言失败时也能释放 gate。
另两个测试覆盖中文与分隔符往返，以及无效状态、重复 ID、截断或损坏文件。

## 综合练习

1. 将取消信号从轮询改成可被唤醒的等待，观察响应与复杂度取舍。
2. 设计可序列化的任务类型和参数，使用 SQLite 恢复未完成任务。
3. 实现带退避的重试，并确认延迟等待不会占满工作线程。

先独立画出所有权和状态提交位置，再对照实现；重点检查取消与完成竞争时的规则。
