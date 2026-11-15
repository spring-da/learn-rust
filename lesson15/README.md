# 第十五课：原子积分转账与幂等回执

目标：扣减、入账、转账回执一起提交；失败时全部回滚，同一次成功转账重试时不重复扣款。

## 启动与演示

先停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson15
$env:DB_PATH = 'E:\WorkProject\rust\lesson15\data\transfers.db'
$env:NOTIFY_DB_PATH = 'E:\WorkProject\rust\lesson15\data\notifications.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
$env:WORKER_ENABLED = '0'
$env:SHUTDOWN_GRACE_MS = '5000'
cargo run --locked
```

使用本课独立数据库，首次为空，重启保留数据，不将上一课文件当作已迁移的数据库。
先暂停注册通知 worker，以便观察账户变化；注册 Outbox 仍会保存。

另开 PowerShell：

```powershell
& 'E:\WorkProject\rust\lesson15\demo.ps1'
```

每次运行创建新账户对并验证：

| 操作 | A / B 积分 | 两边版本 |
| --- | --- | --- |
| 创建账户 | 100 / 20 | 0 / 0 |
| A 向 B 转 30 | 70 / 50 | 1 / 1 |
| 同键同参数重试 | 70 / 50 | 1 / 1 |
| 同键改成转 31 | 409，数据不变 | 1 / 1 |
| 新键请求转 1000 | 409，余额不足 | 1 / 1 |
| 同一个尚未绑定的失败键，修正为转 10 | 60 / 60 | 2 / 2 |

最终 TotalPoints=120；原回执仍保留首次完成后的 70 / 50。
脚本完成后服务继续运行，Ctrl+C 沿用优雅关闭。

## 接口与幂等作用域

```http
POST /transfers
Content-Type: application/json
Idempotency-Key: transfer-001

{"from_user_id":1,"to_user_id":2,"amount":30}
```

首次成功返回 201、Location: /transfers/{id}、Idempotency-Replayed: false。
同键同参数返回同一回执与 Location，仍为 201，重放标记为 true。
GET /transfers/{id} 读取持久化回执；余额与版本是当时的快照。
当前状态用 GET /users/{id} 查询。

| 情况 | 状态 |
| --- | --- |
| 首次成功或成功结果重放 | 201 |
| 成功键用于其他合法参数 | 409 |
| 余额不足、入账会溢出 int64、版本无法推进 | 409 |
| 来源或目标账户不存在 | 404 |
| 自己转自己、ID 或 amount 非正数 | 422 |
| 幂等键缺少/非法、JSON 缺字段/null/类型错误/未知字段 | 400 |
| 请求体过大 / Content-Type 不合要求 | 413 / 415 |

所有数字是 int64 整数，请求体最多 1024 字节。
键规则沿用 1–64 个 ASCII 字母、数字、下划线或短横线。
转账与注册的键分别存储，同一文本在两个接口各自生效。
仅成功提交后绑定转账键，失败后可以继续使用尚未绑定的键提交修正参数。

## 事务边界

```text
BEGIN IMMEDIATE
  查询转账幂等键
    ├─ 已存在：比较参数，重放或报冲突
    └─ 不存在：读取两个账户当前余额和版本
                校验余额与整数边界
                扣减来源账户、版本 +1
                入账目标账户、版本 +1
                保存回执与幂等键
COMMIT 成功后返回结果
```

transfers 的一行保存请求参数和首次成功响应，同时承担回执与转账幂等记录的作用。
如果只把两个余额放进事务，提交后再保存幂等记录，中间失败会让客户端重试时再扣一次。
所以三个写入步骤必须一起提交或一起回滚。
回执保存当时的余额和版本，后来账户改变也不会改变旧回执。

## Rust 的提前返回落在哪里

重点看 src/transfers.rs 的 transfer。
内部 async 块得到 Result<TransferResult, RepositoryError>。
块中的 ? 或 return Err(...) 先结束这个块，然后外层 match 提交或回滚：

```rust
match outcome {
    Ok(result) => {
        tx.commit().await.map_err(RepositoryError::Database)?;
        Ok(result)
    }
    Err(error) => {
        tx.rollback().await.map_err(RepositoryError::Database)?;
        Err(error)
    }
}
```

业务错误与 SQL 错误走相同的事务失败边界。
每次查询都使用 &mut *tx，不通过连接池借另一条连接。
正常错误路径会等待 rollback 完成；任务被取消时，事务对象的 Drop 安排未提交事务的回滚。
已经提交的事务不会因为调用方取消或响应丢失而撤销，所以持久化幂等记录仍然必要。

Go 对照版通过 defer Rollback 与显式 Commit 表达同一个业务边界。

## 余额计算与版本

本课提交的是“转 30”这项业务意图，服务端在事务中读取当前余额，而不是使用客户端提交的目标余额。
客户端无需传 expected_version，但转账会同时推进两个账户版本。
因此另一客户端持有的旧 PUT 版本会失效，不能覆盖转账结果。

BEGIN IMMEDIATE 在读取前取得 SQLite 写事务资格，其他写事务需要等待。
来源 UPDATE 还包含余额足够与已读版本条件，目标 UPDATE 包含入账上限与版本条件。
其他数据库的隔离、行锁和死锁策略需要单独设计，不能直接套用 SQLite 的单写事务假设。

to_points.checked_add(amount) 拒绝入账溢出；余额与版本都使用 i64。
不能依赖先加法后比较来检测溢出，也不能依赖 SQLite 自动改变数值类型。
Go 版用 amount > MaxInt64-toPoints 在加法之前判断。

## 验证

```powershell
Set-Location E:\WorkProject\rust\lesson15
cargo test --locked
```

保留前 26 项测试，新增 8 项覆盖：

- HTTP 回执、键作用域、版本推进与非法请求。
- 两个数据库连接池上的并发同键重试：32 次请求只执行一次。
- 不同键并发扣款：余额 100、每次转 10，只有 10 次成功，其余余额不足。
- 双向并发转账的总量守恒。
- 入账失败、回执写入失败时，两边余额、版本和回执全部回滚。
- 重开数据库后仍返回第一次回执。
- 失败键可重用、账户缺失、整数与版本边界。

总量守恒针对转账操作，已有 PUT 仍允许显式替换积分。
注册 Outbox 继续记录 UserRegistered，本课转账只操作同一个数据库，不调用外部服务。

阅读顺序：schema.sql 的 transfers → src/transfers.rs → src/api.rs 的 create_transfer → src/transfers_tests.rs → demo.ps1。
练习：在回执 INSERT 失败的测试中观察，说明 UPDATE RETURNING 已经返回余额，为什么仍然不能视为转账成功。

参考：[SQLite 事务](https://www.sqlite.org/lang_transaction.html)、
[SQLite UPDATE](https://www.sqlite.org/lang_update.html)、
[SQLite RETURNING](https://www.sqlite.org/lang_returning.html)。
