# 第六课：截止时间与协作式取消

目标：通过可共享的取消标记和截止时间，让正在等待的查询响应停止请求，并在返回前等 worker 退出。
本课的 CancelToken 是自行编写的教学类型，位于 src/cancel.rs。

## 运行与阅读顺序

```powershell
Set-Location E:\WorkProject\rust\lesson06
cargo run --offline
```

只使用标准库。先看 main.rs 的四个 case，再看 cancel.rs、repository.rs，最后看 pool.rs。
本例仍使用 2 个 worker，输入 `[1, 2, 1, 404, 500]`。

## 四种情况

| case | 调用方行为 | 通常观察到的结果 |
|---|---|---|
| normal | 不设置取消与超时 | 约 440ms；正常、未找到、模拟后端失败都保留 |
| timeout | 整批共享 200ms 预算 | 已完成结果保留，其余查询为 deadline-exceeded |
| manual-cancel | 约 100ms 后调用 cancel | 已完成结果保留，其余查询为 cancelled |
| already-cancelled | 调用前已经取消 | 查询入口快速返回 cancelled |

每个 case 都保留每次输入的结果，count=5，重复 ID 与输入顺序继续保留。
任务完成和取消接近同时发生时，已完成结果的具体数量会受调度影响。

## 取消信号包含什么

CancelToken 保存两部分状态：

- AtomicBool：跨线程共享的主动取消标记，初始 false，cancel 时 store(true)，检查时 load。
- Option<Instant>：可选截止时间，创建 token 时确定，之后只读。

本例使用 SeqCst 内存序来读写原子标记，先聚焦跨线程取消通知，内存序规则以后再展开。
取消标记属于共享状态，不能用普通可变 bool 在线程间无同步地读写。
token 被作用域线程借用，作用域保证这些线程结束前它一直有效。

with_timeout 记录统一的截止时刻，所有查询共享它。每次 check 比较 Instant::now() 与该时刻。
预算从创建 token 时开始消耗，不是每个 worker 各获得新的 200ms。
这个教学实现采用检查点观察截止时间；没有另开计时器线程主动唤醒所有等待。

## 怎样让等待响应取消

```rust
token.wait(Duration::from_millis(delay_ms))?;
```

wait 把一次长等待切成最多 10ms 的小段，每段开始时 check：

1. 主动取消则返回 QueryError::Cancelled。
2. 到达截止时间则返回 QueryError::DeadlineExceeded。
3. 等待完成则返回 Ok(())。

这是一种周期性检查的教学实现。10ms 是请求的单次睡眠上限，实际线程唤醒与取消响应仍受调度影响，
所以不能据此保证取消后严格在 10ms 内返回。
repo 在入口先检查 token，排队任务在取消后就能立即返回错误，不再进入模拟等待。
模拟等待之后再次检查，尽量在读取结果前观察到已发生的取消。

实际网络、数据库或异步任务应对接所用库的取消与超时机制，让真正的阻塞点能够响应。
把普通长 sleep 或不可中断 I/O 包在 token 外层，并不能让它们立即停止。

## 错误怎样表示

errors.rs 用一个枚举区分 Cancelled、DeadlineExceeded、BackendUnavailable。
这些错误保存在 LookupResult.outcome 中，已有的成功结果保留。
find_with_pool 外层 Err 仍只表达参数问题，如 workers=0。

本例 token 若主动标记和截止时间都已触发，check 按代码顺序优先报告 Cancelled。
它是这个教学类型自己的规则；Go context 会保留自身实际确定的取消原因。

## 为什么取消后仍要汇总结果

取消函数只修改标记；每个查询在检查点响应它，返回错误后 worker 才能结束当前任务。
本例保留“每个输入一条结果”的契约，所以取消后继续投递、取出和整理剩余任务元数据，
查询本身在入口快速拒绝，最终结果仍有与输入相同的数量。

生产者投递完毕后释放任务发送端，worker 取完队列后退出并释放结果发送端，接收迭代才结束。
thread::scope 在返回前等待生产者与 worker 完成。
因此期限到达是停止信号，函数返回仍需等待任务响应和结果整理；输入很多或底层不配合时会有额外等待。

可以联系 Java 中的中断标记与任务检查来理解这种协作方式。
Go 版通过 ctx.Done() 配合 select 等待信号；本例 Rust 版在小段等待之间检查共享原子标记。

## 动手练习

1. 将超时预算改为 50ms、500ms，观察保留的成功结果以及错误类型。
2. 暂时把 token.wait(...) 换成一次完整的 thread::sleep(...)，保留前后的 token.check()，观察超时后为何仍要等正在执行的慢查询。实验后恢复。
3. 如果要让取消操作立即唤醒模拟等待，可以继续研究 Mutex 配合 Condvar，将周期性检查改为带通知的等待。

Go 对照项目：`E:\WorkProject\go\lesson06`。

## 官方说明

- [AtomicBool](https://doc.rust-lang.org/std/sync/atomic/type.AtomicBool.html)
- [Instant](https://doc.rust-lang.org/std/time/struct.Instant.html)
- [thread::sleep 与调度](https://doc.rust-lang.org/std/thread/fn.sleep.html)
- [thread::scope 等待线程结束](https://doc.rust-lang.org/std/thread/fn.scope.html)
