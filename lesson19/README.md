# 第十九课：接口、trait、静态分发与动态分发（Rust）

案例：两种通知发送器在内存中生成回执，通过计数观察具体调用与实例状态。
本课使用标准库，包含 9 个单元测试与 6 个文档测试。

```powershell
Set-Location E:\WorkProject\rust\lesson19
cargo run
cargo run -- sms
cargo test
```

命令行支持 email 与 sms，默认 email；未知渠道返回错误并以非零状态退出。
Go 配套目录：`E:\WorkProject\go\lesson19`。
src/main.rs 是六组实验；src/lib.rs 是 trait、实现与编译失败例子；src/tests.rs 验证调用、状态和错误路径。

## 实验 1：显式 impl，与借用方式

```rust
pub trait ChannelNamer {
    fn channel(&self) -> &'static str;
}

pub trait Sender: ChannelNamer {
    fn send(&mut self, message: &Message) -> Result<Receipt, SendError>;
    fn fresh(&self) -> Self where Self: Sized;
}
```

`Sender: ChannelNamer` 表示实现 Sender 的类型也必须实现 ChannelNamer。
发送会修改实例计数，所以接收 &mut self；读取渠道只需 &self。
fresh 的限制先留到实验 6 解释。

EmailSender 通过 `impl ChannelNamer for EmailSender` 和 `impl Sender for EmailSender` 显式实现这两个 trait。
仅有同名的固有方法不等于实现 trait，这点与 Go 的隐式实现不同。
[Rust trait 基础](https://doc.rust-lang.org/book/ch10-02-traits.html)

先对具体实例调用 send，再把它借用为 `&mut dyn Sender` 调用，序号为 1、2。
借用没有克隆这个发送器，两个调用修改同一个实例。

## 实验 2：具体类型泛型与 ?Sized

```rust
pub fn send_batch<T: Sender>(sender: &mut T, messages: &[Message])
    -> Result<Vec<Receipt>, SendError>
```

当 T=EmailSender 或 SmsSender 时，编译器按具体类型单态化，send 调用使用静态分发。
`&mut impl Sender` 作为参数也表达泛型约束，与 `&mut dyn Sender` 的动态分发不同。
[Rust 泛型单态化](https://doc.rust-lang.org/book/ch10-01-syntax.html#performance-of-code-using-generics)

默认的类型参数隐含 Sized，不能直接把 `dyn Sender` 用作这里的 T。
配套函数放宽这个约束：

```rust
pub fn send_batch_flexible<T: Sender + ?Sized>(sender: &mut T, messages: &[Message])
    -> Result<Vec<Receipt>, SendError>
```

`?Sized` 表示不再要求 T 一定具有编译期已知大小；T 仍通过引用传入。
它既可接收具体类型，也可让 T=dyn Sender。后一种情况下，send 仍通过 trait object 动态调用。

```rust
let mut sender: Box<dyn Sender> = Box::new(SmsSender::default());
send_batch_flexible(&mut *sender, &messages)?;
```

`*sender` 指向 Box 内的 dyn Sender，`&mut` 借用它；没有把拥有者从 Box 中移出。
批处理遇到错误停止，已成功调用增加的计数保留，测试验证第三条不会在第二条失败后继续执行。

## 实验 3：trait object 与异构集合

```rust
let mut senders: Vec<Box<dyn Sender>> = vec![
    Box::new(EmailSender::default()),
    Box::new(SmsSender::default()),
];
```

Vec 的元素类型统一为 Box<dyn Sender>，各个 Box 内可以拥有不同的具体实现。
trait object 通过数据引用与方法表信息找到具体行为，循环可以使用统一的 send 调用。
[Rust trait object](https://doc.rust-lang.org/book/ch18-02-trait-objects.html)

区分两个独立维度：

| 表达式 | 所有权 | 调用方式 |
| --- | --- | --- |
| `&mut EmailSender`，用作泛型 T | 借用具体实例 | 静态分发 |
| `&mut dyn Sender` | 借用一个实现 | 动态分发 |
| `Box<dyn Sender>` | 拥有其中的实现 | 通过 dyn 进行动态分发 |

动态分发并不要求使用 Box。实验 1 的 `&mut dyn Sender` 直接借用了已有实例；本实验的 Box 还承担集合中实现的所有权。

Dispatch 为每项保存 Result，一项错误不阻止后续项。
测试在两个正常实现之间插入自定义拒绝发送器，确认调用仍会到达最后一项。

## 实验 4：返回 impl Trait 与返回 dyn Trait

| 签名形态 | 含义 |
| --- | --- |
| `fn choose(bool) -> impl Sender` | 隐藏一种具体返回类型，本例两个分支仍必须产生同一类型 |
| `fn make_sender(&str) -> Result<Box<dyn Sender>, SendError>` | 成功时可返回拥有不同实现的同一种 Box<dyn Sender> 类型 |

`impl Sender` 不能自动把 EmailSender 与 SmsSender 的两个返回分支统一。
src/lib.rs 的第一个 compile_fail 测试验证这一点。
工厂使用 Box<dyn Sender>，在运行时选择已经编译进程序的两种实现。

## 实验 5：用 Option 表达缺少实现

```rust
fn optional_sender(enabled: bool) -> Option<Box<dyn Sender>>
```

None 表达没有发送器，Some 持有一个发送器。调用方先匹配 Option，再使用内部实现。
这与 Go 的 nil 接口习惯不同，也不会产生本课 Go 示例中“接口非 nil、内部指针却是 nil”的表达方式。
发送失败则由 send 返回的 Result 表达；“缺少实现”和“本次发送失败”有各自的类型。

## 实验 6：dyn compatibility，旧称对象安全

trait 用作 dyn 时，需要有适合动态调用的接口。
例如，一个没有额外限制的泛型方法 `fn encode<T: Display>(&self, value: T)` 会使这个 trait 无法用于 dyn。
它的类型参数需要按类型实例化，不能直接作为本例这类 trait object 的动态方法入口。

本课 fresh 返回 Self，具体返回类型依赖实现。把它限制为：

```rust
fn fresh(&self) -> Self where Self: Sized;
```

就把 fresh 排除出 trait object 的可调用方法，同时保留通过具体类型调用它的能力。
Sender 整体仍可用于 dyn，send 仍可动态调用；fresh 会创建计数为 0 的同种实现。
[Rust dyn compatibility 规则](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility)

`where Self: Sized` 只加在 fresh 方法上；如果让整个 Sender trait 要求 Sized，它就不能用于 dyn。

## 验证编译器的限制

```powershell
cargo test --doc
```

四个预期编译失败测试分别验证：

1. 本例返回 impl Sender 的两个分支使用不同具体类型。
2. trait 的未限制泛型方法使它不兼容 dyn。
3. 默认 Sized 的泛型函数直接接收 T=dyn Sender。
4. 通过 dyn Sender 调用 fresh。

另两个文档测试验证 ?Sized 的动态调用，以及具体类型调用 fresh 后继续构造 trait object。

## 如何选择

- 调用方已知具体实现，需要保留类型关系：使用具体类型泛型。
- 运行时选择实现、统一保存不同实现：使用 trait object。
- 借用现有对象：可以使用 &dyn 或 &mut dyn；需要拥有实现时再选择 Box 等拥有者。
- 多数方法要动态调用，个别方法只适合具体类型：可通过方法上的 Self: Sized 划分调用能力。

Java 泛型的类型擦除、Go 的编译器实现选择与 Rust 单态化应分别理解。性能取舍需要结合实际负载验证。

## 练习

1. 增加 PushSender，显式实现两个 trait，并加入异构集合。
2. 去掉 fresh 方法上的 Self: Sized，观察创建 Box<dyn Sender> 时的编译错误。
3. 比较 send_batch 与 send_batch_flexible 的调用，说明为什么“函数使用泛型”仍不足以判断内部一定是静态分发。

学习目标：从签名判断需要哪些能力、是否保留具体类型，以及数据由谁拥有。
