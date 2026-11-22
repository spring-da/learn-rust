# 第 24 课：可恢复任务、所有者与执行令牌

本课把第 23 课的终态文件推进为 SQLite 任务存储，持久化任务类型、参数和中间状态。
第 25、26 课通过 Cargo path dependency 复用它，请保留相邻目录。

```powershell
Set-Location E:\WorkProject\rust\lesson24
cargo run --locked
cargo test --locked
```

首次运行需要下载、编译 bundled SQLite 依赖，之后可用 --offline。
默认每次创建新的 data/recovery-*.db；查询指定文件：`cargo run --locked -- --db '数据库绝对路径' --list`。
打开数据库就会执行启动恢复；不要将它指向旧课程的数据库。

## 持久化的是数据，不是闭包

Spec 保存 kind、payload、max_attempts、fail_until、work_ms、delay_ms，重启后由执行器解释这些参数。
kind 为 uppercase 或 flaky；文本为 1～512 个 UTF-8 字节；总尝试预算 1～8；模拟耗时最多 2000ms；首次延迟最多 60000ms。
flaky 的 fail_until 用来模拟前几次执行失败。

Store 持有 Mutex<Connection>，不同线程通过它串行使用 rusqlite 连接。
事务、Statement 与连接的释放遵循各自的所有权，SQL 错误通过 Result 传播。
schema.sql 与 Go 完全一致。

## Submit、Claim、Finish 各负责什么

| 操作 | 数据库保证 |
| --- | --- |
| submit | Immediate 事务中检查键、参数和容量，再插入 |
| claim | 一条条件 UPDATE 完成领取、attempts+1、run_token+1 |
| finish | 只更新仍为 running 且令牌匹配的记录 |
| cancel | 排队任务直接终止；运行任务持久化取消请求，由提交结果或恢复处理 |

重放幂等键返回同一任务的当前状态，参数不同冲突。未完成容量统计 queued + running。
一条原子 SQL 领取避免两个 worker 同时拿到同一轮任务。
[SQLite 事务](https://sqlite.org/lang_transaction.html)、[RETURNING](https://sqlite.org/lang_returning.html)

## 恢复规则

Store::open 取得独占 owner 后，在向调用方返回之前处理遗留 running：

- 有取消请求：cancelled。
- 已耗尽预算：failed。
- 否则：queued，下一次立即可领取。

attempts 不会回退，run_token 再增加一次，使旧尝试失去提交资格。
默认演示先模拟未完成提交，再重新打开：旧 token=1 失效，恢复 token=2，重新领取 token=3、attempts=2。
单元测试也确认新尝试已经 running 时，旧令牌仍被拒绝。

## Drop 与 owner 锁

Store 把数据库连接与保持文件锁的 File 一起拥有，字段按连接、owner 的顺序声明，连接先释放。
File::try_lock 获取锁，进程结束会释放操作系统锁；磁盘上的 .owner 文件仍可保留。
因此不能用“文件是否存在”判断运行状态，也不应通过删除 owner 文件抢占。
[File 文件锁](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)

Go/Rust 使用同一路径协议，已验证跨语言互斥。当前模型是一个 owner 进程、多个 worker，其他进程不能在它执行期间重置 running 状态。
多 owner 服务需要租约、心跳和失效接管，属于另一个设计问题。

## 至少一次执行仍需要幂等

WAL 与 synchronous=FULL 保护 SQLite 提交，不能把外部操作与 finish 合并成一个事务。
外部操作完成后、数据库结果提交前退出，可能导致恢复后再次执行。
run_token 防止过期结果覆盖当前状态，也不会替外部系统撤销已经发生的动作。

六个测试覆盖恢复、预算、取消、owner、幂等、容量、并发领取与时间边界。
整组还实际强制结束过 Go 进程，再由 Rust 接续同一任务：attempts=2、run_token=3，结果正确。

## 练习

1. 改成 max_attempts=1，再运行中断演示，解释恢复策略。
2. 增加一种参数化任务，并设计数据库与执行器的版本兼容。
3. 解释为什么“独占 File 锁 + 数据库条件更新”同时存在。

整组入口：[LEARNING_PATH.md](LEARNING_PATH.md)。
