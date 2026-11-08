# 第五课：worker、有限队列与 Mutex 锁作用域

目标：固定查询线程数量，用有限任务队列调度，再按输入顺序返回每个结果。

## 运行

```powershell
Set-Location E:\WorkProject\rust\lesson05
cargo run --offline
```

只用标准库。先看 src/pool.rs，再看 src/main.rs；repository.rs 沿用只读仓库和模拟等待。
每次函数调用创建一组作用域线程；同一批次里一个 worker 可以连续执行多个任务，整批完成后线程结束。

输入 `[1, 2, 1, 404, 500]`，分别使用 1 和 2 个 worker。
两次都应按以下顺序返回全部 5 条结果，包括重复 ID：

```text
  index=0 id=1 found=Spring
  index=1 id=2 found=Lin
  index=2 id=1 found=Spring
  index=3 id=404 not-found
  index=4 id=500 error=simulated backend unavailable
```

随后输出 `empty: count=0` 和 workers=0 的参数错误。
等待总和为 840ms；2 个 worker 的模拟任务调度通常约 440ms，再加执行开销。
实际时间随系统调度变化，不以固定时长或固定加速倍数为正确性标准。

## 本例的执行结构

1. 一个生产者线程把带 index 和 id 的 Job 发送到有限队列，发完后释放 jobs_tx。
2. 固定数量的 worker 轮流取任务，各自完成查询并发送一个 LookupResult。
3. 调用方持续接收结果，结果发送侧全部释放后迭代结束，再按 index 排序。
4. scope 返回前，等待其中的生产者和 worker 都结束。

workers=2 限制的是同时执行查询的 worker 数。程序还包含生产者和调用线程。
输入少于 workers 时，实际 worker 数减少到输入数。

## 为什么需要 Mutex<Receiver<Job>>

标准库 mpsc 支持多个发送者和一个接收端；Receiver 不能像 Sender 那样 clone，且自身不是 Sync。
本例用 Mutex 保护这个接收端，让多个 worker 轮流取得它的访问权。
继续使用 thread::scope 借用本地 Mutex，因此可以直接传引用。

```rust
let next = {
    let receiver = jobs_rx.lock().expect("job receiver lock is not poisoned");
    receiver.recv()
};
```

lock 返回 MutexGuard。通过 guard 才能访问被保护的数据；guard 离开作用域时自动释放锁。
这里借助一个小代码块，让 guard 在 recv 完成后就被释放。
查询和结果发送都在这个锁作用域外进行，所以不同 worker 可以同时处理各自的任务。

若让 guard 一直存活到慢查询结束，下一个 worker 就无法取任务，实际查询会被串行化。
可以联系 Java 的 synchronized 小代码块，或 try/finally 中的 unlock 来理解临界区边界。

recv 在队列暂时为空时会阻塞，此时其他 worker 等待互斥锁；生产者发送任务不需要这把锁。
所有任务发完后，队列里的消息仍可取走；消息取完且发送侧断开时，recv 返回 Err，worker 退出循环。

lock 的 expect 对应本例内部“持锁部分没有 panic”的不变量；正常仓库故障通过结果里的 Err 传回。

## 有限队列与背压

```rust
let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(workers);
```

队列容量限制等待缓存的任务数。队列满时，生产者 send 阻塞，直到有 worker 取走任务。
下游处理速度会反过来约束上游投递速度，这就是背压。
容量与 worker 数是独立参数，本例将它们设成同一个值以减少配置。
把容量改成 1 或 0，执行查询的并发上限仍由 worker 数控制。

生产者与结果汇总同时运行，避免所有线程都等待对方先收消息。
输入和最终结果仍保留整批数据，所以有限任务队列不意味着总内存不随输入规模增长。

## 顺序与重复 ID

index 表示输入位置，id 表示业务标识；两个相同 id 也有不同 index。
先接收全部结果，再按 index 排序，保留重复任务与每个任务的错误。
完成顺序可以不同，但返回结果的顺序与输入一致。

当前排序为 O(n log n)。也可以用 Vec<Option<LookupResult>> 表示 n 个待填充槽位，
由汇总方按 index 写入，再检查并取出所有结果，实现 O(n) 汇总。

## 两层 Result

find_with_pool 的外层 Err 表示参数不合法，如 workers=0。
返回 Vec 中每个 LookupResult 的 outcome 则表示该用户查询的成功、未找到或错误。
一个查询失败仍会产生一条结果，其余任务继续进行。
workers 为 usize，因此类型本身排除了负数；仍需显式检查零。
本例先检查 workers，再处理空输入，所以零 worker 搭配空输入也返回参数错误。

## 动手练习

1. 改用 3、10 个 worker，观察结果顺序和实际 worker 数。
2. 把任务队列容量改为 1，解释并发上限为何仍由 worker 数决定。
3. 将排序换成 Vec<Option<LookupResult>> 的按 index 填充，同时验证重复 ID 和错误结果完整。

Go 对照项目：`E:\WorkProject\go\lesson05`。

## 官方说明

- [Receiver 的线程约束](https://doc.rust-lang.org/std/sync/mpsc/struct.Receiver.html)
- [Mutex 与 guard](https://doc.rust-lang.org/std/sync/struct.Mutex.html)
- [sync_channel 与容量](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html)
