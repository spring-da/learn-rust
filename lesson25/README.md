# 第 25 课：持久化延迟重试与可测试调度

本课通过 path dependency 复用 ../lesson24，把执行一轮的逻辑与后台循环分开。

```powershell
Set-Location E:\WorkProject\rust\lesson25
cargo run --locked
cargo test --locked
```

默认演示用 Manual 时钟，快速观察 0、10、30ms 的状态变化，数据库在新的 data/schedule-*.db 中。

## 调度步骤的边界

step 接收 Store、Policy、Clock、随机输入与本次执行闭包：

```text
领取到期任务 → 执行一次 → 提交成功 / 写回未来重试时间
```

闭包只执行一次，所以参数要求 FnOnce。整轮结束后，worker 可以处理其他任务。
重试间隔写在 next_run_at 中，不能让失败任务持有 worker 睡完整个退避时间。

演示输出：任务1在 time=0 失败，due=10；任务2仍在 time=0 完成；任务1在 10 再失败、due=30，最后在 30 成功。
第 26 课才使用真实后台 runner；本课先让调度规则可以确定性地验证。

## Policy 与时钟注入

```text
ceiling = min(cap, base × 2^(attempt-1))
low     = max(1, ceiling/2)
delay   = low + entropy % (ceiling-low+1)
```

示例采用 equal jitter 形式，并限制最大延迟。固定 entropy 后，每次测试能得到同样的结果。
Clock trait 返回毫秒数，Manual 可以推进而无需 sleep；RealClock 使用实际 Unix 时间。
[退避与 jitter](https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/)

持久化 queued 任务的 next_run_at 在重启后继续保留；running 的恢复策略来自第 24 课。
真实 runner 没有到期任务时短暂等待 10ms；这是一种简单轮询，不是高精度定时器。

## Arc、停止控制与数据库生命周期

Runner 的线程各自拥有 Arc<Store>，共享同一存储；实际数据库连接由 Mutex 保护。
Control 可以被 HTTP 层克隆，用于请求停止与读取后台错误；线程句柄仍由 Runner 拥有并 join。

| 结果 | 行为 |
| --- | --- |
| 成功 | finish 为 succeeded |
| 模拟业务失败 | 根据预算排队重试或 failed |
| interrupted | 不提交业务结果，留下 running 供恢复 |
| 数据库等 Error | 停止调度并记录后台错误 |

普通业务失败由 Outcome.failure 表达，基础设施错误由 Result 的 Err 表达。
Builtin 只转换 ASCII 小写字母；中文与 ß 保持不变，使 Go/Rust 的结果规则一致。
work_ms 等待中会检查取消与停止；没有把等待放进 SQLite 事务或连接锁范围内。

停止顺序：请求 runner 停止 → join worker → 释放其 Arc → 释放最后一个 Store。
这同时应用了前面几课的所有权、Drop 和跨线程共享。

六个测试覆盖退避上限与随机范围、其他任务不受退避占用、重启期限、取消、中断恢复及停止。

## 练习

1. 为业务失败增加可重试/永久失败的明确分类。
2. 实现可唤醒的调度等待，避免固定间隔轮询。
3. 解释为什么后台线程使用原子停止标记，而数据库结果提交仍需要 run_token 条件。
