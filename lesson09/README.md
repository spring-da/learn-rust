# 第九课：SQLx、SQLite 持久化与约束

目标：用异步数据库仓库承接 HTTP 请求，理解连接池、参数绑定、结果映射和数据库唯一约束。

## 启动

```powershell
Set-Location E:\WorkProject\rust\lesson09
cargo run
```

默认监听 `127.0.0.1:8081`，数据库位于本项目的 `data/users.db`，首次创建时为空。
SQLx 启用 Tokio 运行时和 bundled SQLite；首次构建会下载依赖并通过已有的 Windows C++ 工具链编译 SQLite。
不需要单独安装 SQLite 数据库服务。

下载慢时可只为本次命令使用镜像：

```powershell
cargo --config 'source.crates-io.replace-with="rsproxy-sparse"' --config 'source.rsproxy-sparse.registry="sparse+https://rsproxy.cn/index/"' run
```

镜像参数依据 [RsProxy 说明](https://rsproxy.cn/)，不会修改全局配置。

另开 PowerShell 创建并读取用户：

```powershell
$response = Invoke-WebRequest -Uri 'http://127.0.0.1:8081/users' -Method Post -ContentType 'application/json' -Body '{"name":"Alice"}'
$location = $response.Headers.Location
$response.StatusCode
$response.Content
Invoke-RestMethod -Uri ('http://127.0.0.1:8081' + $location)
```

停止并在同一项目目录重新启动服务后，继续读取相同 Location，数据仍应存在；再次创建同名用户返回 409。
新数据库没有预置数据，使用实际返回的 id 或 Location，不沿用前几课的固定 ID。

启动前可设置 `$env:HTTP_ADDR = '127.0.0.1:18081'` 与 `$env:DB_PATH = 'data/another.db'`。
相对 DB_PATH 根据启动工作目录解析。要重复使用数据，应保持数据库路径一致。
data/ 被 gitignore 排除，Cargo.lock 保留具体依赖版本。

## 阅读顺序

先看 schema.sql，再看 repository.rs 的 open、find_by_id、create，最后看 main.rs 和 api.rs 的 await 调用。
`include_str!("../schema.sql")` 在编译时把 SQL 放入程序，运行时无需查找独立 schema 文件。
本课先用 CREATE TABLE IF NOT EXISTS 初始化表，后续再做版本化迁移。

## 参数与结果

```rust
let (id, name): (i64, String) = sqlx::query_as(
    "INSERT INTO users (name) VALUES (?) RETURNING id, name"
)
.bind(name)
.fetch_one(&self.pool)
.await?;
```

bind 将名字作为参数传递，RETURNING 返回新记录。
query_as 将返回列映射为声明的元组类型，再构造拥有数据的 User。
find_by_id 使用 fetch_optional，正常的“没找到”得到 None，而 SQL 执行错误进入 Err。
可以联系 Java 的 JDBC 参数绑定、ResultSet 映射和连接池理解这些职责。

这里使用运行时查询函数；这组函数不需要在编译时连接数据库。
API 的 DTO、字段校验和状态码约定沿用第八课，仓库调用现在通过 await 等待数据库结果。

## 数据库约束与错误转换

schema.sql 使用 `name TEXT NOT NULL UNIQUE`。
Create 直接插入，不在应用内先查重再保存；所有连接都由同一数据库约束把关。
From<sqlx::Error> 检查数据库报告的 is_unique_violation，转换为 RepositoryError::NameExists，最后返回 HTTP 409。
其他数据库错误保留原始原因并交给 API 映射为存储故障。

数据库生成主键，应用无需维护 next_id。ID 不保证连续无空洞。
本例用单条 INSERT 创建一条记录；多条业务语句的一致提交和回滚留到下一课。

## 连接池与异步

SqlitePool 长期持有并复用连接，本例每个仓库最多 4 个连接。
Tokio 调度等待数据库结果的 future；SQLite 本身执行同步操作，SQLx 的 SQLite 驱动通过后台工作线程承接这些操作。
这延续了前一课的原则：异步接口如何完成等待，仍取决于底层驱动实现。

开启 WAL 并配置 5 秒 busy_timeout。SQLite 同一时间仍只允许一个写事务执行，连接池不是并行写入数量的承诺。
超出锁等待或获取连接的等待范围时，仍可能发生数据库错误。
测试显式 pool.close().await 后再重新打开数据库，保证连接资源完成关闭。

仓库不再包含 RwLock<内存数据>。多连接访问时的唯一性由数据库约束保证，Arc 继续用于共享仓库所有权。

## 验证

```powershell
cargo test
```

需要镜像时，在 test 前加入与 run 相同的 --config 参数。
测试覆盖 HTTP 契约、关闭后重新打开、含单引号名字的参数绑定，以及两个独立连接池同时创建同名用户。
并发检查发起 32 次创建，要求 1 次成功、31 次唯一约束冲突；测试使用临时目录，不写默认数据库。

练习：启动两个实例，使用同一个绝对 DB_PATH、不同 HTTP_ADDR；
从一个实例创建用户，再从另一个实例读取，并验证重复名字得到 409。

Go 对照项目：`E:\WorkProject\go\lesson09`，默认端口 8080。

## 官方说明

- [SQLx](https://docs.rs/sqlx/latest/sqlx/)
- [SqliteConnectOptions](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)
- [连接池](https://docs.rs/sqlx/latest/sqlx/struct.Pool.html)
- [SQLite UNIQUE](https://www.sqlite.org/lang_createtable.html#unique_constraints)
- [SQLite RETURNING](https://www.sqlite.org/lang_returning.html)
