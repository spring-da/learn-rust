# 第二十课：闭包、迭代器与订单报表（Rust）

目标：过滤已付款且金额不少于 1000 分的订单，转换为展示行，按客户统计数量，再观察错误如何结束处理。
本课使用标准库，包含 10 个单元测试与 6 个文档测试。

```powershell
Set-Location E:\WorkProject\rust\lesson20
cargo run
cargo test
```

Go 配套目录：`E:\WorkProject\go\lesson20`。
src/main.rs 包含六组实验，src/lib.rs 是模型、报表与文档测试，src/tests.rs 验证行为。

## 连接 Java Stream 经验

`iter().filter(...).map(...).collect()` 与你熟悉的 Stream 流水线相似：先描述处理，再消费元素。
Rust 还要求明确每个元素是借来的、可修改的，还是被移动出来的。
迭代器不是已经存满结果的容器，构造中间适配器不等于完成全部处理。

## 实验 1：捕获方式与闭包调用能力

```rust
let mut minimum = 1000_i64;
let fixed = move |order: &Order| order.amount_cents >= minimum;
minimum = 2000;
```

minimum 是 Copy 类型，这里 move 闭包捕获一份 1000，外部变量仍可更新为 2000。
对于 String 等非 Copy 类型，按值捕获可能移动拥有者；只借用时，借用规则继续适用。

Fn、FnMut、FnOnce 的区别在于怎样调用闭包和使用捕获值：

| 约束 | 调用闭包时的接收方式 | 本课例子 |
| --- | --- | --- |
| Fn | 共享借用闭包，类似 &self | 持有 String，每次读取 len |
| FnMut | 独占借用闭包，类似 &mut self | 每次给捕获的计数器加 1 |
| FnOnce | 按值消费闭包，类似 self | 把捕获的 String 移出闭包体 |

所有闭包至少实现 FnOnce；Fn 可以用于要求 FnMut 或 FnOnce 的位置，FnMut 也可以用于要求 FnOnce 的位置。
如果只知道参数满足 FnOnce，调用方只能依赖调用一次的能力。
[Fn](https://doc.rust-lang.org/std/ops/trait.Fn.html)、[FnMut](https://doc.rust-lang.org/std/ops/trait.FnMut.html)、[FnOnce](https://doc.rust-lang.org/std/ops/trait.FnOnce.html)

**move 不等于只能调用一次。** 捕获 String 后只调用 len 的闭包仍可实现 Fn；把 String 从闭包体返回的闭包则只实现 FnOnce。
Fn 约束也不等于所有可达数据都不可改变，例如共享借用的 Cell 仍可通过内部可变性记录计数。
[Rust 闭包说明](https://doc.rust-lang.org/book/ch13-01-closures.html)

paid_at_least 返回 `impl Fn(&Order) -> bool`：它隐藏一个具体闭包类型，捕获本次 minimum。
这连接了上一课 impl Trait 的含义。EagerLabels 的 Rust 版本接收 FnMut，以允许循环反复调用带可变状态的谓词。

## 实验 2：三种迭代方式决定元素权限

| 对 Vec<Order> 的操作 | 每次产生的 Item | 原 Vec |
| --- | --- | --- |
| orders.iter() | &Order | 共享借用，仍由原拥有者持有 |
| orders.iter_mut() | &mut Order | 独占借用，可修改元素 |
| orders.into_iter() | Order | 对这个拥有的 Vec 按值转换，原变量被移动 |

本课先读取 ID，再通过 iter_mut 把第一笔金额从 1200 改成 1300，最后用 into_iter 把每个 Order 的客户 String 移出来。
IntoIterator 也可为引用类型实现，因此要看接收者究竟是 Vec 还是 &Vec，不能只看方法名就断定数据一定被移动。
[IntoIterator](https://doc.rust-lang.org/std/iter/trait.IntoIterator.html)

Iterator 的关联类型 Item 描述 next 每次产出的值；同一逻辑对象通过不同迭代方式，可以产生不同权限的 Item。
这是上一课 trait 在标准库中的一个直接应用。

## 实验 3：过滤、转换、收集

```rust
let result: Vec<String> = orders.iter()
    .filter(|order| order.paid && order.amount_cents >= 1000)
    .map(format_order)
    .collect();
```

结果依次为 `#1 Alice 1200`、`#3 Alice 2500`、`#5 Bob 1800`。
iter 的 Item 是 &Order，而 filter 的谓词接收 &Item，因此这里谓词参数实际为 &&Order；自动解引用支持直接访问字段。
filter 输出的 Item 仍为 &Order，map 中的 format_order 接收的就是 &Order。

qualifying 返回借用 orders 的惰性迭代器。生命周期把输出与输入切片的借用关联起来；最低金额由 move 闭包按值保存。
创建链时，参数表达式仍会求值，也会发生需要的移动或借用；惰性主要指逐元素的过滤、转换等操作推迟到消费时执行。
[Rust 迭代器说明](https://doc.rust-lang.org/book/ch13-02-iterators.html)

过滤与转换适配器不收集中间 Vec，但 format_order 会创建 String，collect 会创建结果 Vec；不能由“惰性”推断没有分配。

## 实验 4：消费推进迭代器的位置

流水线创建后，过滤与转换计数都是 0。
消费 take(2) 后，过滤 3 次、转换 2 次，因为第 2 条输入不符合条件。
main.rs 用 Cell 记录单线程计数，让程序能在闭包仍存在时读取这些数值。

Rust 本例的迭代器持有当前位置。collect 按值消费该迭代器变量，不能直接再 collect 一遍。
需要从中途接着读时，可以临时借用：

```rust
let mut cursor = orders.iter().map(|order| order.id);
let head: Vec<_> = cursor.by_ref().take(2).collect(); // [1, 2]
let rest: Vec<_> = cursor.collect();                 // [3, 4, 5, 6]
```

要从头开始处理这份 Vec，可以重新创建 orders.iter() 及其适配器。
Go 对照案例基于可重遍历的 Seq，每次消费重新执行源函数；这两份例子的重复消费行为不同。
[Iterator 与 by_ref](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.by_ref)

## 实验 5：fold 聚合

group_counts 以空 BTreeMap 为累加器，对每个合格订单更新客户数量：Alice 为 2，Bob 为 1。
fold 的闭包拿到当前累加器与元素，返回更新后的累加器。
BTreeMap 使展示顺序按客户键排序；Go 版本使用 map 后显式排序。

可以把它对应到 Java Stream 的 groupingBy 与 counting，但本课保留逐步更新的写法，便于看到累加器的数据流。

## 实验 6：collect 的目标类型决定错误策略

parse_amounts 先 map，每个元素产生 `Result<i64, AmountError>`，再收集成：

```rust
Result<Vec<i64>, AmountError>
```

全成功时返回 Vec；遇到首个 Err 时立刻结束并返回错误，不返回部分成功列表。
输入 `["1200", "bad", "1800"]` 只尝试两次解析，错误位置为下标 1。
parse_amounts 内部已经调用 collect，因此调用这个函数本身就会执行处理。

先前观察回调的副作用仍已发生，返回 Err 不会自动回滚它们。
如果业务需要跳过错误行，就应明确设计另一种策略；简单把 Err 丢弃会丢掉失败信息。

## 文档测试与练习

`cargo test --doc` 验证四个预期编译失败例子：只实现 FnOnce 的闭包调用两次、FnMut 传给 Fn 参数、Vec into_iter 后使用旧变量、collect 后再次使用原迭代器变量。
另外两个通过的例子验证 move 闭包仍可实现 Fn，以及 by_ref 后继续遍历。

1. 把最低金额改为 1800，预测输出与分组。
2. 对照 `move || label.len()` 和 `move || label`，解释为什么调用能力不同。
3. 将 iter 改成 into_iter，检查 map 的元素类型与后续还能使用哪些变量。

完成标准：从闭包体判断调用能力，从 Item 判断元素权限，并说明消费动作何时发生。
