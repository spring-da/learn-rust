# 第十课：SQLx 事务、回滚与资源生命周期

目标：原子地创建用户与积分账户，在错误路径等待回滚完成，再向 HTTP 调用方返回业务结果。

## 启动和演示

先停止上一课服务，再运行：

```powershell
Set-Location E:\WorkProject\rust\lesson10
$env:DB_PATH = 'data/registration.db'
cargo run
```

默认监听 `127.0.0.1:8081`，使用本课独立数据库。首次使用为空，后续重启仍保留已提交数据。
依赖与上一课一致，Cargo.lock 继续锁定版本。

在另一个 PowerShell 先触发第二条 SQL 的约束错误：

```powershell
'{"name":"RollbackDemo","initial_points":-1}' | Set-Content -Encoding ascii request.json
curl.exe -i http://127.0.0.1:8081/users -H 'Content-Type: application/json' --data-binary '@request.json'
```

预期 422：`{"error":"initial points must be non-negative"}`。
保持同名，改用合法积分重试：

```powershell
'{"name":"RollbackDemo","initial_points":100}' | Set-Content -Encoding ascii request.json
curl.exe -i http://127.0.0.1:8081/users -H 'Content-Type: application/json' --data-binary '@request.json'
```

预期 201，响应为包含 id、name、points 的对象。使用实际 Location 查询，能读到 points=100。
重试能成功说明第一次失败没有留下占用名字的用户。
请求文件中的名字是 ASCII 示例，其他语言的名字应使用 UTF-8 JSON。

initial_points 缺失或 null 时默认为 0；非整数或超出 i64 的值返回 400。
同名用户仍返回 409。负积分只在第二条 INSERT 的 CHECK 处拒绝，便于演示真实回滚。

官方源下载慢时可仅为本次命令使用镜像：

```powershell
cargo --config 'source.crates-io.replace-with="rsproxy-sparse"' --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' run
```

此配置依据 [RsProxy 说明](https://rsproxy.cn/)，不修改全局 Cargo 配置。

## 表结构和业务边界

users 保存身份；points_accounts 保存 user_id 与非负积分。
user_id 的主键保证每个用户最多一个账户，外键保证账户引用存在的用户。
创建事务把两条写入作为一个整体提交，GET 通过 JOIN 返回用户与积分的聚合响应。
外键与事务分别保护引用关系和注册操作的整体一致性。

本课故意让积分非负规则由数据库 CHECK 执行。以后可在请求层提前验证，数据库约束仍保留。
CHECK 默认拒绝当前语句；事务层负责把之前成功执行的用户 INSERT 一并回滚。

## 阅读 Create 的三个部分

1. pool.begin().await 创建事务，取得并持有一个池连接。
2. async 块通过同一个 tx 执行两条 INSERT，得到 Result<User, sqlx::Error>。
3. 成功分支等待 commit，错误分支等待 rollback，再返回结果或映射后的错误。

```rust
.fetch_one(&mut *tx)
```

这表示通过事务底层连接执行查询。tx 解引用为连接，再进行可变借用。
两条 SQL 都使用这一连接，才能处在同一事务内。
如果改为向 &self.pool 执行，连接池会另行安排连接，这不能表达当前事务边界。

内部 async 块中的 ? 把语句错误交回 outcome，外层仍拥有 tx，可以明确等待 rollback().await。
commit 和 rollback 都消费事务对象；完成后不能继续使用同一个 tx。

## Drop 的回滚保障

SQLx 的 Transaction 在未结束时被丢弃，会启动回滚清理。
Drop 不能直接 await；具体清理由驱动安排。正常的业务错误路径中，本例显式 rollback().await，
以便在返回错误前等回滚完成，并处理回滚本身可能出现的数据库错误。
如果整个 future 被取消等路径导致事务被丢弃，Drop 仍提供最后的清理保障。

## 错误映射和提交结果

唯一约束冲突变成 NameExists，再返回 409。
CHECK 冲突变成 InvalidPoints，再返回 422；本例只有积分非负这一项 CHECK。
新增其他 CHECK 后，需要更精确地映射约束来源。
只有 commit 成功才返回用户，其他数据库错误按仓库故障处理。

事务内已经得到的 id 和 points 只是提交前的操作结果，成功响应以提交结果为准。
调用方超时或断开连接时，最终结果还与提交进度有关；后续会用幂等案例继续处理重试问题。

## 验证

```powershell
cargo test
```

镜像场景可在 test 前加与 run 相同的 --config 参数。
测试覆盖：正常提交后两表都有记录、关闭再打开后积分保留、第二条 SQL 失败后两表均为空、
同名修正后重试成功、跨连接池并发创建只提交一组记录，以及 HTTP 请求契约。
验证回滚时直接统计物理表，避免 JOIN 隐藏半成品造成误判。

练习：增加 registration_events 表，并把第三条事件 INSERT 放在同一事务中。
设计一个事件约束错误，验证第三步失败也会撤销前面的用户和积分账户。

Go 对照项目：`E:\WorkProject\go\lesson10`，默认端口 8080。

## 官方说明

- [SQLx Transaction](https://docs.rs/sqlx/latest/sqlx/struct.Transaction.html)
- [raw_sql 执行固定建表脚本](https://docs.rs/sqlx/latest/sqlx/fn.raw_sql.html)
- [SQLite 外键](https://www.sqlite.org/foreignkeys.html)
