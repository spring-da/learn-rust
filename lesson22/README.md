# 第 22 课：对照测试、基准与性能判断

案例：统计 queries 中每次查询是否属于 allowed。优化前扫描切片，优化后查 HashSet。
allowed 中重复 ID 不增加命中数，queries 中重复查询要重复计数。

```powershell
Set-Location E:\WorkProject\rust\lesson22
cargo run --offline
cargo test --offline
cargo bench --offline --bench membership
```

## 正确性先于性能

三个测试覆盖空集合、重复查询、负数、索引独立性，并对 1000 组确定性生成的数据比较两种算法。
这个生成测试使用固定种子，便于复现；它不是覆盖引导的 fuzz 工具。
Go 配套案例另外演示了原生 fuzz。

## 在稳定版上运行自定义基准

Cargo.toml 的 `[[bench]]` 配置 `harness = false`，由 benches/membership.rs 的 main 执行测量。
cargo bench 使用优化后的 bench profile；本课没有依赖 nightly 的 #[bench] 或外部基准库。
[Cargo bench 说明](https://doc.rust-lang.org/cargo/commands/cargo-bench.html)

每种方案先热身，再收集 5 个样本，每个样本 8 次调用；每轮轮换方案次序，并取中位数。
black_box 用于减少无用结果删除和常量折叠等优化，属于尽力而为的编译器提示。
[black_box](https://doc.rust-lang.org/std/hint/fn.black_box.html)

| 方案 | 测量范围 |
| --- | --- |
| linear | 每次在线性切片中查找 |
| indexed_reuse | HashSet 提前创建，只测查询 |
| build_and_count | 每次创建 HashSet 并查询 |

本机 Rust 1.99.1、Windows amd64、4096 个 allowed、10000 次 queries 的一次观察：

| 方案 | 中位数 μs/op |
| --- | ---: |
| linear | 772.188 |
| indexed_reuse | 35.538 |
| build_and_count | 57.575 |

这是小型教学基准，不包含完整置信区间或分配统计。重跑与 HashSet 种子、机器负载等都会影响数值。
Go 与 Rust 的测量框架、实现和优化路径不同；这些数字用于评估各自方案，不用于语言排名。

## 如何定位和解释

先读 count_linear：每个 query 都可能扫描 allowed，再看 count_indexed 的一次集合查询。
对照规模变化、测量建索引的成本，就能检查性能差异是否符合算法结构。
Go 配套课实际生成 CPU profile，让你继续验证“扫描循环是否就是热点”。

Rust 的 contains 可能得到编译器优化；不能仅根据源代码行数推断最终指令成本。
完整服务还应选择代表性的生产或模拟负载，用合适的平台分析工具定位后再修改。

## 练习

1. 把 allowed 从 4096 改成 4、64、65536，记录构造成本与查询成本。
2. 实现多批查询共用一个 HashSet，并测整个使用周期。
3. 在保持重复查询语义的前提下，尝试别的数据结构，再用同一组测试比较。

验收：先说明被测算法的输入与输出约定，再说明优化的数据结构、代价与可复用范围。
