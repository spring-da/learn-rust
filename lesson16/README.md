# 第十六课：泛型、TTL 与有界回执缓存

目标：用泛型与 trait bounds 写可复用缓存，理解 Clone、所有权、单调时钟与短时互斥锁，再接入回执查询。

## 启动与演示

停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson16
$env:DB_PATH = 'E:\WorkProject\rust\lesson16\data\cache.db'
$env:NOTIFY_DB_PATH = 'E:\WorkProject\rust\lesson16\data\notifications.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
$env:WORKER_ENABLED = '0'
$env:CACHE_CAPACITY = '2'
$env:CACHE_TTL_MS = '2000'
cargo run --locked
```

独立数据库首次为空，重启保留业务数据；进程内缓存重启后为空。
服务默认容量 64、TTL 2 秒。演示设容量为 2，方便观察第三个回执加入时的淘汰。
CACHE_CAPACITY 支持 1–1024，CACHE_TTL_MS 支持 1–60000。

另开 PowerShell：

```powershell
& 'E:\WorkProject\rust\lesson16\demo.ps1'
```

脚本要求容量 2、TTL 在 500–5000 毫秒，依次验证：

| 操作 | X-Receipt-Cache |
| --- | --- |
| 首次 GET 回执 | MISS |
| 再次 GET | HIT |
| 再次转账后读取旧回执 | HIT，仍是原快照 |
| 手动删除缓存项，再读 | MISS |
| 等待 TTL，再读 | MISS |
| 放入第三份回执，再读最早项 | MISS，FIFO 已淘汰最早项 |

原回执来源积分保持 70，三次转账后的当前来源积分为 59。

## 为什么选择不可变回执

回执行表示一次已提交转账的结果，后续转账不会修改它。
旧回执的 70 是快照，当前余额用 GET /users/{id} 查询数据库。
余额校验、扣减、入账仍在数据库事务中执行，不读取缓存来判断能否转账。
缓存只加速 GET /transfers/{id}，不预热 POST 响应，也不缓存 404 或数据库错误。

## Rust 的约束为什么分开写

src/cache.rs 定义 TtlCache<K,V>。方法按实际需要分别设置约束：

| 约束 | 用途 |
| --- | --- |
| K: Eq + Hash | HashMap 查找与相等判断 |
| K: Clone | 插入时让键同时存在于 HashMap 和 FIFO 队列 |
| V: Clone | 读取时返回拥有所有权的值，释放锁后仍可安全使用 |

```rust
let receipts = TtlCache::<i64, TransferReceipt>::new(2, Duration::from_secs(2))?;
let labels = TtlCache::<String, String>::new(2, Duration::from_secs(2))?;
```

它们共享实现，但具体类型与可调用方法在编译期确定。
V: Clone 只加在 get_at 所在的 impl；写入和删除不因为读取需要 Clone 就无条件要求 V 可复制。
返回 Option<V> 表达有值或未命中，不用空字符串、0 或 null 混淆合法值与缺失。

API 用 Arc<TtlCache<i64,TransferReceipt>> 共享同一份缓存。
Arc::clone 增加共享所有者，并不复制所有条目；get_at 中的 V::clone 才复制返回的值。
本课回执由整数构成，返回后可以独立使用。
Clone 是类型定义的复制行为；若 V 本身包含 Arc，克隆也可能仍共享内部数据，不承诺通用深拷贝。

Rust 编译器会对泛型使用具体类型进行单态化；具体机制可对照 Rust Book 的 generics 章节。
Go 版使用 K comparable 约束 map 键，V any 不代表把值存成动态 any，也不需要运行时类型断言。

## TTL 与 FIFO

使用 Instant 与 Duration 表达经过时间，不使用日期时间或 Unix 时间做缓存期限。
put_at 保存 now.checked_add(ttl) 的截止点；不能表示的极端时间范围返回错误。
get_at 在 now < expires_at 时命中，等于截止点就过期。
读取不续期，覆盖写入同键才重置期限。
显式传入 Instant 使边界测试不必真的等待两秒。

FIFO 淘汰最早写入的项，命中读取不会调整顺序，因此不是 LRU。
插入先清理过期项，再检查容量；重复写同一个键先删除旧顺序记录，避免 VecDeque 无限增长。
条目数与队列元数据都有上限，清理与顺序调整为 O(capacity)。
这是按条数而不是字节计算的容量。过期采用惰性清理，不另起后台任务。

## Mutex 为什么可以用于这个异步接口

缓存内部用 std::sync::Mutex 保护 HashMap、VecDeque 和计数。
方法内只做短时内存操作，返回值拥有所有权，MutexGuard 不会跨 await。

```text
get_at：短锁，命中则返回克隆值
未命中：锁已释放，await 数据库
put_at：再用短锁写入结果
```

不是把整个异步数据库调用放进 std::sync::Mutex。
连接池占满的测试会让数据库查询等待，同时检查 /cache/stats 仍能完成。
并发未命中可以重复回源，本课未加入相同键的请求合并。

## API 与统计

| 接口 | 作用 |
| --- | --- |
| GET /transfers/{id} | 成功响应附 HIT/MISS 请求缓存标记 |
| GET /cache/stats | entries、capacity、ttl_ms、hits、misses、evictions、expirations、database_loads |
| DELETE /cache/receipts/{id} | 返回 204，只删除本进程缓存项 |
| GET /users/{id} | 查询数据库当前账户状态 |

database_loads 统计回执数据库读取的尝试次数，包括未找到与失败。
原子计数的 Relaxed 用于指标，不用作业务同步协议。
两个进程各自持有缓存和统计，手动删除一端的缓存不会操作另一端。
数据库回执持久存在，缓存过期、被淘汰或进程重启后可以再次加载。

## 测试与练习

```powershell
Set-Location E:\WorkProject\rust\lesson16
cargo test --locked
```

保留前 34 项测试，新增 9 项验证：多种类型实例、返回值所有权、TTL 边界、FIFO 与过期优先清理、
重复覆盖的元数据上限、配置和时间范围、并发访问、HTTP 删除/过期/快照、404 不缓存，
以及等待数据库时缓存锁已经释放。

阅读顺序：src/cache.rs → src/api.rs 的 get_transfer → src/main.rs → src/cache_tests.rs → demo.ps1。
练习：说明为什么 get_at 返回 Option<V> 而不是锁内的 &V；再分别解释 K: Clone 与 V: Clone 是哪项操作需要的。

参考：[Rust 泛型](https://doc.rust-lang.org/book/ch10-01-syntax.html)、
[Trait bounds](https://doc.rust-lang.org/book/ch10-02-traits.html)、
[Instant](https://doc.rust-lang.org/std/time/struct.Instant.html)。
