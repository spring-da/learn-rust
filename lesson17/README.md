# 第十七课：slice 与 Vec 的内存行为（Rust）

本课为标准库控制台实验：6 组运行观察、7 个单元测试、3 个文档测试。
其中两个文档测试确认代码因所有权或借用冲突而无法编译，一个确认正确调整顺序后可以编译运行。

```powershell
Set-Location E:\WorkProject\rust\lesson17
cargo run
cargo test
```

配套 Go 实验：`E:\WorkProject\go\lesson17`。
src/main.rs 包含运行实验；src/lib.rs 包含通用辅助函数、编译失败示例及测试。
可单独运行 `cargo test --doc` 验证文档中的三个例子。

## 先区分拥有者与视图

| 类型 | 含义 | 能否直接改变集合长度 |
| --- | --- | --- |
| `Vec<T>` | 拥有一块可增长的连续元素存储 | 绑定可变时可以 push |
| `&[T]` | 对已有元素的共享借用视图 | 不能 push |
| `&mut [T]` | 对已有元素的独占借用视图，可以修改元素 | 不能 push |
| `&mut Vec<T>` | 对 Vec 拥有者的独占借用 | 可以 push，调用方看到新长度 |

Go 的 slice 同时提供数组视图与 append 操作；Rust 把拥有者 Vec 与借用视图分别表达。
Vec 的长度表示已初始化的元素数，capacity 表示可用容量；预留容量不会创建可索引的元素。
本课使用 i32 等非零大小元素，不讨论零大小类型的特殊容量行为。

## 实验 1：赋值是移动所有权

```rust
let original = vec![10, 20, 30];
let owner = original;
// original 随后不能使用；owner 持有原来的元素缓冲区。
```

移动不克隆元素。main.rs 保存移动前的地址用于观察，移动后 owner 指向同一块缓冲区。
对于 `[i32; 3]` 这类实现 Copy 的数组，赋值会复制值，两份数组都能继续使用。
[Rust 所有权说明](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html)

可把 Vec 想成具有唯一所有者的一组“指针、长度、容量”信息；实际字段顺序没有固定承诺。
移动后不会留下两个同时负责释放同一 Vec 缓冲区的拥有者。

## 实验 2：借用部分元素

```rust
let mut points = vec![10, 20, 30];
let prefix: &[i32] = &points[..2];
println!("{prefix:?}"); // prefix 的最后一次使用

let writable: &mut [i32] = &mut points[..2];
writable[0] = 99;
println!("{writable:?}"); // writable 的最后一次使用

points.push(40);
```

视图共享元素存储，没有克隆元素。修改 writable[0] 会修改 points[0]。
这些借用在最后一次使用后即可结束，不必总等到右花括号。
如果把最后使用的位置挪到 push 后，借用冲突就可能使程序无法编译。

## 实验 3：push 与扩容

main.rs 先用 `Vec::with_capacity(5)` 预留容量，放入 3 个整数，再 push 第 4 个：容量足够，缓冲区保留。
另一组先填满实际 capacity，再 push，确认容量增加且旧元素保留。
扩容后的地址可能改变，也可能保持原值，因此程序只观察地址，不断言地址一定改变。
具体增长倍数也没有固定保证。[Vec 容量及保证](https://doc.rust-lang.org/std/vec/struct.Vec.html#capacity-and-reallocation)

**为什么有容量仍不能“拿着元素引用，再 push，再使用引用”？**

```rust,compile_fail
let mut points = Vec::with_capacity(100);
points.extend([10, 20, 30]);
let first = &points[0];
points.push(40);
println!("{first}");
```

push 接收 `&mut self`，与后续使用的 first 的共享借用冲突；编译器不会因本次容量充足就取消这个规则。
通用规则也避免了扩容后引用指向失效存储的问题。
可以先完成 first 的使用，或用 `let first = points[0]` 复制本例的 i32 值。
[Rust Book 的 Vec 借用解释](https://doc.rust-lang.org/book/ch08-01-vectors.html)

src/lib.rs 将这段代码和“移动后使用旧变量”分别放入 compile_fail 文档测试。
`cargo test` 中这两个测试通过，表示编译器正确拒绝了它们。

## 实验 4：创建独立拥有的副本

```rust
let base = vec![10, 20, 30];
let mut copied = base[..2].to_vec();
copied[0] = 99;
copied.push(77);
// base:   [10, 20, 30]
// copied: [99, 20, 77]
```

这里 i32 元素独立。与 Go `base[:2:2]` 相比，to_vec 已经创建新 Vec 并复制了元素；
Go 的限制容量表达式仍共享当前元素，只在后续追加超出容量时分配新数组。

## 实验 5：Clone 取决于元素的语义

本课 `snapshot<T: Clone>(&[T]) -> Vec<T>` 使用 to_vec，它为每个元素调用 Clone。
两组例子对照：

- User 的字段是 String 和 Vec<String>，派生 Clone 会克隆这些字段；修改副本的名字和标签不会改动原值。
- `Vec<Rc<Cell<i32>>>` 克隆的是 Rc 句柄，两份 Vec 的元素指向同一个 Cell；副本 set(99)，原值也能读到 99。

Rc 表达单线程共享所有权；Cell 让本例能修改共享整数。先观察结果即可，下一阶段再深入这些类型。
Clone 不保证任意数据结构都递归复制成独立对象图。[Clone 的官方语义](https://doc.rust-lang.org/std/clone/trait.Clone.html)

上一课缓存的 `V: Clone` 使返回值拥有自己的 V，但 V 内部的 Arc、Rc 等仍可能共享数据。
因此更换缓存值类型时，需要一起检查该类型的 Clone 实现。

## 实验 6：函数参数表达修改权限

```rust
fn append_point(points: &mut Vec<i32>, point: i32) {
    points.push(point);
}

let mut points = vec![10, 20];
append_point(&mut points, 77);
// points 是 [10, 20, 77]
```

只读取元素时优先接收 `&[T]`；只修改现有元素时优先接收 `&mut [T]`；
需要 push 或改变拥有者结构时，接收 `&mut Vec<T>`。
main.rs 的 show 接收 `&Vec<i32>`，是因为它专门观察 Vec 的 capacity。

## 动手练习

1. 把 lib.rs 第二个编译失败例子的 `let first = &points[0]` 改成复制整数值，预测编译结果。
2. 将实验 2 的 `println!("{writable:?}")` 放到 push 之后，解释错误产生的位置。
3. 对照 Go 的嵌套 slice 副本，解释 Vec<User> 与 Vec<Rc<Cell<i32>>> 的克隆结果为什么不同。

完成标准：能说清每个变量是在拥有数据、借用数据，还是持有共享所有权句柄。
