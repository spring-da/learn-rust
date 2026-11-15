# 第十四课：乐观锁与并发修改积分

目标：检测客户端是否基于过期数据修改积分，避免后一条写入覆盖前一次修改，并正确处理冲突后的读取与重算。

## 启动与演示

先停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson14
$env:DB_PATH = 'E:\WorkProject\rust\lesson14\data\optimistic.db'
$env:NOTIFY_DB_PATH = 'E:\WorkProject\rust\lesson14\data\notifications.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
$env:WORKER_ENABLED = '0'
$env:SHUTDOWN_GRACE_MS = '5000'
cargo run --locked
```

本课新增账户版本与注册响应快照字段，使用独立数据库，不把第十三课文件当作已迁移后的数据。
为观察积分先暂停 Outbox worker，注册事件仍然持久保存。

另开 PowerShell：

```powershell
& 'E:\WorkProject\rust\lesson14\demo.ps1'
```

脚本每次创建一个新用户和请求键，然后模拟：

```text
A、B 先读取 points=100、version=0
A 加 20：目标 120、expected_version=0 → 200，新版本 1
B 减 20：目标 80、expected_version=0 → 409，数据不变
B 重新读取 120、version=1，重算目标 100 → 200，新版本 2
```

最终 FirstUpdate=200、StaleUpdate=409、RecalculatedRetry=200、FinalPoints=100、FinalVersion=2。
RegistrationReplayVersion=0 表示原注册请求仍重放最初快照。
演示完成后服务继续运行，Ctrl+C 沿用上一课的关闭流程。

## 乐观锁保护的是什么

数据库把两次无条件 UPDATE 串行执行，仍然可能把最终积分写成 80。
因为 B 的 80 是从旧的 100 计算而来，覆盖了 A 的加 20。
客户端的读取与提交是两次独立请求，单次写入原子性无法识别这种旧数据。

本课为 points_accounts 增加非负整数 version，初始为 0。
积分替换同时检查旧版本并递增新版本：

```sql
UPDATE points_accounts
SET points = ?, version = version + 1
WHERE user_id = ? AND version = ?
  AND version < 9223372036854775807
RETURNING user_id, points, version;
```

比较与修改由同一条 SQL 完成。不能先在应用里读版本、比较，再无条件写入。
所有业务写入都需要遵守这个协议；绕过它直接更新积分会破坏版本检测。
数据库中的版本条件同时约束多个连接、Go 进程与 Rust 进程，不依赖一份内存中的锁。

与 Java/JPA 的 @Version 对照：都通过版本变化发现读取后的并发修改，
本课通过手写 SQL 和 Rust 错误类型显式实现。

## 接口契约

```http
PUT /users/1/points
Content-Type: application/json

{"points":120,"expected_version":0}
```

成功响应：

```json
{"user_id":1,"points":120,"version":1}
```

GET /users/1 和注册响应的用户对象新增 version 字段。
PUT 接收目标积分值，演示中的加减由客户端根据当前积分计算。
Idempotency-Key 继续作用于 POST /users，PUT 不使用注册接口的重放记录。

| 情况 | HTTP 状态 |
| --- | --- |
| 合法参数且版本匹配 | 200，替换积分并推进版本 |
| 版本不匹配 | 409，不修改账户 |
| 用户不存在 | 404 |
| points 或 expected_version 为负数 | 422 |
| 缺字段、null、非整数、超出 int64、未知字段 | 400 |
| 超过 1024 字节 | 413 |
| Content-Type 不合要求 | 415 |

成功写入相同积分仍然推进版本。版本达到 int64 上限时返回 409，防止版本加一溢出并变成其他数值类型。

## Rust 的 Option 与 Result 各表达什么

主要阅读 src/points.rs 的 replace_points。

```rust
let row: Option<(i64, i64, i64)> = sqlx::query_as("UPDATE ... RETURNING ...")
    .bind(points)
    .bind(id)
    .bind(expected_version)
    .fetch_optional(self.pool())
    .await
    .map_err(RepositoryError::Database)?;
```

该片段展示调用结构，实际 SQL 在源文件中完整列出。
外层 Result 表达数据库执行是否出错；内层 Option 表达条件更新有没有匹配行：

- Ok(Some(...))：更新成功，拿到当次写入后的积分与版本。
- Ok(None)：没有匹配行，补查区分不存在、版本冲突与版本耗尽。
- Err(...)：真正的数据库执行错误，不伪装成乐观锁冲突。

补查只负责错误分类，决定是否能写入的是先前那条条件 UPDATE。
本课没有用户删除接口，因此没有并发删除改变存在性的业务路径。
单条 UPDATE 自身是原子的，不需要让客户端的 GET 与后续 PUT 占用一个长事务。

RepositoryError::VersionConflict 经 match 映射为 HTTP 409，UserNotFound 映射为 404。
请求 DTO 的 points、expected_version 是必填 i64，由 Serde 拒绝缺失与 null；合法的 0 会被接受。
Go 对照版使用 *int64 在解码后区分缺失、null 与 0。

## 409 后不能只改版本号

B 若只把 expected_version 改成 1，继续提交旧的 points=80，版本检查会通过，
但会覆盖 A 的结果。若业务意图是扣 20，应重新读取并计算 120-20=100。
是否自动重试取决于业务是否允许重新应用这项意图。
人工编辑冲突通常还需要让调用者决定如何合并。

并发递增测试使用有上限的“读取 → 计算 → 条件更新 → 冲突则重试”。
测试中的 yield_now 让其他任务获得执行机会；重试上限避免无限争抢。
生产代码需要结合负载选择退避与冲突处理方式。

## 与此前案例衔接

请求幂等识别“是不是同一次操作的重试”，乐观锁检测“是不是使用过期状态提交修改”。
更新到 version=2 后，原注册请求仍会重放注册时 points=100、version=0。
继续修改当前账户前应 GET 最新状态，而不是依赖注册快照。
UserRegistered 事件也保持注册时的积分，本课没有增加其他事件类型。

## 验证与阅读顺序

```powershell
Set-Location E:\WorkProject\rust\lesson14
cargo test --locked
```

保留前 20 项测试，新增 6 项：HTTP 冲突和重算、非法请求不改账户、
两个连接池的 32 个同版本写入只有一个成功、16 个并发递增经重新读取重试后全部保留、
版本边界，以及 SQL 失败不推进版本且成功更新重启可读。
原注册快照测试也改为通过版本接口修改积分。

阅读顺序：schema.sql → src/points.rs → src/api.rs 的 replace_points → src/points_tests.rs → demo.ps1。
练习：把 B 重试的目标积分故意设为 80，解释为什么版本检查通过仍不能保证客户端的计算意图正确。

参考：[SQLite UPDATE](https://www.sqlite.org/lang_update.html)、
[SQLite RETURNING](https://www.sqlite.org/lang_returning.html)、
[Jakarta Persistence @Version](https://jakarta.ee/specifications/persistence/3.2/apidocs/jakarta.persistence/jakarta/persistence/Version.html)。
