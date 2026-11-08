# 第四课：线程、借用与结果传递

目标：理解标准库作用域线程、Sync、move 闭包、无缓冲 channel 和发送端生命周期。
沿用仓库接口与 Result<Option<User>, E>；仓库使用内存数据和 sleep 模拟远程查询等待。
本课只使用标准库，不需要下载第三方依赖。

## 运行与阅读顺序

```powershell
Set-Location E:\WorkProject\rust\lesson04
cargo run
```

先看 src/repository.rs，再对照 src/batch.rs 的两个批量函数，最后看 src/main.rs。

主要输出示例：

```text
sequential: count=4 elapsed=...ms
  id=1 found=Spring
  id=2 found=Lin
  id=404 not-found
  id=500 error=simulated backend unavailable
concurrent: count=4 elapsed=...ms
  id=2 found=Lin
  id=500 error=simulated backend unavailable
  id=404 not-found
  id=1 found=Spring
```

并发结果按接收顺序收集，具体顺序不作保证。打印发生在批量函数返回后。
模拟等待分别是 240、80、160、120 毫秒，串行通常约 600 毫秒，并发约最长等待的 240 毫秒，再加运行开销。
时间受系统调度影响，不把固定时长或固定加速倍数当作正确性标准；这也不是 CPU 密集计算性能测试。

## 标准库线程与作用域

`scope.spawn(...)` 创建标准库线程，使用 OS 线程模型。
Go 的 goroutine 则由 Go runtime 调度，两者不能简单视为相同成本的执行单元。

`thread::scope` 保证其中启动的线程在 scope 返回前被等待完成。
所以这些线程可以借用外部的本地仓库，而仓库不会在它们仍使用它时被销毁。
这延续了前几课的规则：引用的使用时间必须受到被引用值有效期的约束。

## 为什么仓库 trait 增加 Sync

```rust
pub trait UserRepository: Sync { ... }
```

Sync 表示共享引用能够安全地跨线程使用；精确说，T 为 Sync 时 &T 为 Send。
本例线程共享借用同一仓库，仓库内部 HashMap 在任务启动后只读。
编译器会根据字段和类型实现推导 MemoryUserRepository 的 Sync 能力。
这个约束允许线程接收 &dyn UserRepository；去掉 Sync 后，这个 trait 对象就无法提供所需保证。

可以暂时移除 trait 声明中的 `: Sync`，运行 cargo check，阅读报错后恢复。

## move 究竟移动了什么

```rust
let tx = tx.clone();
scope.spawn(move || { ... });
```

每次迭代复制一个发送端句柄，将这个句柄交给当前线程。
move 让闭包按值捕获所使用的外部变量。
其中 tx 的所有权被转入闭包；id 是 Copy 的整数；repo 是 Copy 的共享引用，复制引用仍指向同一仓库。
因此 move 不意味着把整个仓库复制或移走。

`for &id in ids` 从切片元素的引用中取得一个复制后的整数。
`&[i64]` 是对一段连续整数的切片借用，可以接收本例数组的视图。

## 两种 clone 的用途

`tx.clone()` 复制发送端句柄，多个句柄连接同一条通道，不会复制通道里的消息。
`self.users.get(&id).cloned()` 则把 Option<&User> 转成 Option<User>。
User 的 Clone 实现由 derive 生成，会克隆其中的 String，使查询结果拥有自己的字符串。
所以 Clone 的具体成本和行为由类型实现决定，不能把所有 clone 都理解成同一种操作。

## channel 何时结束

`mpsc::sync_channel(0)` 创建无缓冲通道，一次发送需与接收方配合。
它返回独立的发送端 tx 和接收端 rx；本例允许多个发送端、一个接收端。
普通 `mpsc::channel()` 则是无界缓冲通道，与这里的容量 0 不同。

流程如下：

1. 每个任务拥有自己的 tx，完成后发送一个 LookupResult，然后释放它的发送端。
2. drop(tx) 主动释放父线程保留的原始发送端，它本身不负责发送。
3. 当前线程持续接收；当所有发送端均被释放且结果已取完时，接收迭代结束。
4. scope 返回前等待其中的线程结束，最终返回 Vec<LookupResult>。

如果父线程始终持有原始 tx，接收迭代就无法仅凭任务已完成来判断不会再有消息。
scope 只有在闭包结束后才完成自动等待，因此这种接收等待也不能靠 scope 自动解除。

这里 send 的 expect 用来表达“接收方在收集期间保持存活”的内部不变量。
正常的查询失败作为 LookupResult.outcome 中的 Err 传回，不会触发这个 expect。

## 数据与结果

多个查询共享只读仓库，每个 LookupResult 拥有查询结果。
发送结果时，它的所有权交给通道接收侧；当前线程统一收集到 Vec。
本例错误是固定的模拟消息，使用 &'static str，继续保留成功、未找到与失败三种状态。

## 动手练习

1. 把 ids 改成空数组 []，再运行，两个函数都应返回 0 条结果。
2. 重复一次 ID 1，确认并发结果也包含两次查询，不能按 ID 静默去重。
3. 把 sync_channel(0) 的容量改为 ids.len()，理解发送方何时会阻塞。
4. 若要求按输入顺序返回，先描述如何把输入下标随结果一起发送，再由汇总方重排。

当前输入规模固定且很小，每个 ID 启动一个线程。批量规模扩大时，下一步可用 worker pool 限制并发数。

Go 对照项目：`E:\WorkProject\go\lesson04`。

## 官方说明

- [thread::scope](https://doc.rust-lang.org/std/thread/fn.scope.html)
- [Sync](https://doc.rust-lang.org/std/marker/trait.Sync.html)
- [sync_channel](https://doc.rust-lang.org/std/sync/mpsc/fn.sync_channel.html)
