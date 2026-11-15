# 第十一课：用事务实现请求幂等

目标：客户端因超时而重试注册请求时，返回第一次成功的结果，同时避免重复创建用户和积分账户。

## 为什么用户名唯一还不够

第十课保证用户和积分账户一起提交。但提交成功后的 HTTP 响应可能丢失。
客户端重试后仅得到“用户名已存在”，无法确定第一次请求的结果。

本课由客户端为一次业务操作生成 Idempotency-Key，重试时沿用该键。
新操作使用新键。服务端把成功结果持久化，重启后仍可重放。

## 启动

先停止占用 8081 的上一课服务，在 PowerShell 中运行：

```powershell
Set-Location E:\WorkProject\rust\lesson11
$env:DB_PATH = 'E:\WorkProject\rust\lesson11\data\idempotency.db'
$env:HTTP_ADDR = '127.0.0.1:8081'
cargo run --locked
```

本课使用独立数据库，首次启动为空；后续重启继续使用同一文件。
请求与响应仍沿用第十课的格式。

## 动手请求

在另一个 PowerShell 中运行：

```powershell
'{"name":"Alice","initial_points":100}' | Set-Content -Encoding ascii request.json
curl.exe -i http://127.0.0.1:8081/users -H 'Content-Type: application/json' -H 'Idempotency-Key: reg-001' --data-binary '@request.json'
```

首次响应为 201，包含 Location、Idempotency-Replayed: false，以及用户的 id、name、points。
原样再执行一次 curl，仍为 201，返回同一用户和 Location，Idempotency-Replayed 变为 true。

保留 reg-001，把 initial_points 改成 200 再发：得到 409，错误为
`idempotency key was used with different parameters`。
改用 reg-002，保持用户名 Alice：得到 409，错误为 `name already exists`。

| 请求 | 结果 |
| --- | --- |
| 新键、新名字、合法积分 | 201，创建并保存结果 |
| 同键、相同规范化参数 | 201，重放第一次的结果 |
| 同键、不同规范化参数 | 409，拒绝键被重新用于另一操作 |
| 不同键、已存在的名字 | 409，用户名唯一约束仍然生效 |
| 缺少键或键格式不合法 | 400 |
| 新键、负积分 | 422，回滚全部写入，不占用键 |

Idempotency-Key 必须恰好一个值，由 1–64 个 ASCII 字母、数字、下划线或短横线组成。
这是本课 API 自己规定的接口契约。GET 不要求此请求头。

按解析后的参数比较：name 去掉两端空白；initial_points 省略或为 null 时为 0。
JSON 字段顺序和空白不影响判定；`{"name":" Alice "}` 与
`{"initial_points":0,"name":"Alice"}` 表示相同参数。
本课只绑定成功提交的请求；失败后修正积分，可以继续使用尚未绑定的键。

## 事务中的三次写入

users 和 points_accounts 保存当前业务数据。
新增 idempotency_records 保存键、规范化请求参数，以及首次成功响应的字段快照。

```text
BEGIN IMMEDIATE
  查询 Idempotency-Key
  ├─ 已存在：相同参数 → 返回原快照；不同参数 → 键冲突
  └─ 不存在：INSERT users
              INSERT points_accounts
              INSERT idempotency_records
COMMIT 后才返回成功
```

如果先提交用户，再独立保存幂等记录，中间失败就会出现“业务已经完成，但查不到重试结果”。
三次写入必须一起提交或一起回滚。
不要把发邮件、远程调用等耗时操作放进这个持有 SQLite 写锁的事务。

快照不会随当前账户变化。首次注册 points=100，之后账户变成 150：
GET 返回 150，而原注册请求的重放仍返回 100。

## Rust 中的关键实现

按 src/api.rs → src/repository.rs → schema.sql 阅读。

API 从 HeaderMap 校验键，解析并规范化请求，再调用仓库。
RegistrationResult 包含 user 与 replayed，handler 据此设置响应头。
RepositoryError::KeyMismatch 表达同键不同参数，区别于用户名冲突 NameExists。
二者映射为不同错误消息，HTTP 状态都为 409。

仓库通过以下调用在读取幂等记录前取得写入资格：

```rust
let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
```

SQLx 的事务持有连接，查询和写入都通过 `&mut *tx` 执行。
SQLite 同时只允许一个写事务；本例让不同连接、不同服务进程的“查键后写入”依次执行。
如果普通延迟事务先读后写，升级为写事务时可能遇到 SQLITE_BUSY。
busy_timeout 给锁竞争设置等待时间，并不保证任何负载下都成功。
换数据库时，需要根据新的隔离与锁语义重新设计。

内部 async 块得到 `Result<RegistrationResult, RepositoryError>`。
块内的 `?` 提前结束这个内部块，然后外层 match 明确 commit 或 rollback：

```rust
match outcome {
    Ok(result) => {
        tx.commit().await?;
        Ok(result)
    }
    Err(error) => {
        tx.rollback().await?;
        Err(error)
    }
}
```

与 Java 的事务业务方法对照：这里用 Result 表达成功或失败，用事务对象显式确定提交边界。
若任务被取消，事务对象的 Drop 仍负责安排未完成事务的回滚；正常错误路径在返回前显式等待回滚完成。
Go 版通过 defer Rollback 和显式 Commit 表达同一个业务边界。

## 验证

```powershell
Set-Location E:\WorkProject\rust\lesson11
cargo test --locked
```

测试覆盖请求头规则、参数规范化、HTTP 重放和冲突、失败后修正重试，以及：

- 重开数据库后仍能重放，账户变化不会改变原响应快照。
- 用触发器让第三次写入失败，直接检查三张表都为空。
- 两个连接池并发提交 32 个同键请求，仅一次创建，其余 31 次返回相同结果。
- 不同键争用同一用户名，仍然只创建一份用户和账户。

与 Go 服务联合演示时，在启动两个服务前，把两个终端的 DB_PATH 都设为同一个绝对路径，例如
`E:\WorkProject\go\lesson11\data\idempotency.db`。
随后在 8080 创建，用相同键和参数向 8081 重试，可以读到同一份结果。

## 本课的边界与练习

键只作用于本例 POST /users，在接口内全局唯一，不自动过期；未涉及登录用户与租户。
真实系统需要根据业务确定调用方、接口、键的作用域与保留时间。
本课只覆盖同一数据库事务中的写入，不能据此保证外部邮件、支付或消息也只执行一次。

练习：沿着“事务提交成功 → 响应丢失 → 客户端重试”的路径走读 create，说明为什么返回同一个 id；
再找出第三次 INSERT 失败时，`?` 返回到哪里、rollback 在哪里执行。

参考：[SQLite 事务](https://www.sqlite.org/lang_transaction.html)、
[SQLx Pool::begin_with](https://docs.rs/sqlx/0.9.0/sqlx/struct.Pool.html#method.begin_with)、
[SQLx Transaction](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html)。
