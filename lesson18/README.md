# 第十八课：所有权、借用、生命周期与资源释放（Rust）

案例：读入 UTF-8 文档，借用第一行作为预览，需要独立保存时创建 String，并观察资源释放顺序。
本课使用标准库，包含 9 个单元测试和 6 个文档测试。

```powershell
Set-Location E:\WorkProject\rust\lesson18
cargo run
cargo test
```

默认读取本目录 sample.txt。可执行 `cargo run -- '文件的绝对路径'` 读取第一行以 `TITLE:` 开头的文档。
配套 Go 目录：`E:\WorkProject\go\lesson18`。

| 文件 | 阅读重点 |
| --- | --- |
| src/main.rs | 六组运行实验 |
| src/lib.rs | Document、生命周期签名、文件读取、四个预期编译失败案例 |
| src/cleanup.rs | Drop、作用域、提前返回和事件记录 |
| src/tests.rs | 文本有效性、拥有/借用 reader、释放顺序与错误路径 |

## 实验 1：返回拥有数据的值

Document 内部保存 String，new 接收 String 的所有权，再把 Document 移动给调用方。
函数结束后，调用方继续拥有文本；移动不要求克隆这段字符串内容。

```rust
pub fn new(text: String) -> Self {
    let doc = Self { text };
    doc
}
```

如果函数创建局部 String，再返回指向其内容的 &str，局部拥有者会在返回时销毁，引用无法继续有效。
src/lib.rs 第一个 compile_fail 例子展示这个错误。返回拥有数据的 String 或 Document 可以满足这种需求。
[Rust 借用规则](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html)

## 实验 2：借用行与独立拥有的行

```rust
let mut doc = Document::new(String::from("TITLE: old\nbody"));
let borrowed = doc.first_line();
let owned = borrowed.to_owned();
println!("{borrowed}"); // borrowed 的最后一次使用
doc.replace_text(String::from("TITLE: new"));
println!("{owned}");    // 仍为 TITLE: old
```

| 表达式 | 结果 |
| --- | --- |
| `doc.first_line()` | &str，借用 Document 内部文本 |
| `borrowed.to_owned()` | String，拥有复制出的文本 |
| `doc.replace_text(...)` | 需要 &mut self，与仍在使用的共享借用不兼容 |

把 borrowed 的打印移到 replace_text 后面，就会出现借用冲突。
owned 则可以在 doc 替换或销毁后继续使用。

借用在最后使用后可以结束；拥有资源的局部值通常仍要等到所属作用域结束才 Drop。
“引用不再使用”与“拥有者已经释放”需要分别判断。

## 实验 3：生命周期标注表达什么

first_line 只有一个输入引用，返回引用关联到它，通常可以省略标注。
方法 first_line(&self) 的返回引用则关联到 self 的借用。

choose_preview 返回值可能来自 text，也可能来自 fallback，所以显式表达关系：

```rust
pub fn choose_preview<'a>(text: &'a str, fallback: &'a str) -> &'a str {
    let line = first_line(text);
    if line.is_empty() { fallback } else { line }
}
```

`'a` 表示：选取一个两份输入借用都有效的范围，返回值在这个范围内使用。
两个拥有者的实际存活范围可以不同。标注不会延长局部变量的寿命，也不会复制数据。
即使一次调用实际选中了 text，输出也必须遵守这个函数签名对两份输入的共同约束。
[Rust 生命周期说明](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)

```text
text 拥有者       ─────────────────────────
fallback 拥有者          ───────────
本例返回的借用            ─────────
```

需要跨出短期输入的作用域保存结果时，可在输入仍有效时对结果调用 to_owned。
src/lib.rs 包含一个错误案例与一个不同实际存活范围的正确案例。

## 实验 4：Drop 在内层块结束时执行

cleanup_scope 创建两个命名的 TraceResource 局部变量：

```rust
let _outer = TraceResource::open("outer", trace);
{
    let _inner = TraceResource::open("inner", trace);
    trace.record("body:inner");
}
trace.record("body:outer");
```

输出：

```text
open:outer → open:inner → body:inner → close:inner → body:outer → close:outer
```

内层绑定结束作用域时，inner 的 Drop 已经执行。Go 对应示例的 defer 会等到函数退出。
本课这些命名局部资源按作用域清理，同一作用域按绑定声明的逆序释放。
下划线开头的 `_inner` 仍是一个命名绑定，用来持有资源。
[Drop 说明](https://doc.rust-lang.org/book/ch15-03-drop.html)

TraceResource 是演示用的模拟资源，Drop 只记录事件。
Trace 中的 Rc<RefCell<...>> 用来让这些资源共同写入单线程日志，与文件关闭实现无关。

## 实验 5：提前返回与显式 drop

early_return 用 `?` 传播校验错误。正常返回与 `?` 提前返回都会清理当前函数已经拥有的局部资源。
不需要在每个错误分支重复写清理代码。

`drop(resource)` 按值接收资源，移动所有权并使它在该调用中释放；随后不能再次使用原变量。
它可以让释放发生在所属块结束之前，不应直接调用 Drop trait 的 drop 方法。
[std::mem::drop](https://doc.rust-lang.org/std/mem/fn.drop.html)

Java 的 try-with-resources 可帮助理解“把资源的使用范围和清理绑定起来”；Rust 通过拥有者与 Drop 表达这个关系。

## 实验 6：拥有 File 和借用 File 的区别

load_document 打开 File，并按值传入 read_document。
read_document 把内容读入 String，校验后返回 Document；File 在函数返回时已释放，文本继续由 Document 持有。

```rust
pub fn read_document(mut reader: impl Read) -> io::Result<Document>
```

注意泛型参数实际是什么类型：

- 传入 File：函数拥有 File，返回时释放它。
- 传入 `&mut File`：函数只拥有一个借用；返回结束借用，File 仍由调用方拥有。

测试用带 Drop 计数的 reader 验证了这两种情况，也验证成功、无效 UTF-8、标题错误和读取失败的清理。

File 的 Drop 会关闭文件，但关闭错误不会通过 Drop 返回给调用者。
写入等需要报告失败的操作，应通过相应的显式操作和 Result 处理；不能把“已执行 Drop”当作“所有业务操作已成功”。
本课读取案例中的 read_to_string 和校验错误会正常传播。[File 文档](https://doc.rust-lang.org/std/fs/struct.File.html)

## 验证编译失败案例

```powershell
cargo test --doc
```

其中四个测试确认编译器拒绝：返回局部内容引用、借用超过 Document 的作用域、借用仍使用时替换文本、返回结果超过短期 fallback 的范围。
另外两个测试确认独立拥有 String 与在共同有效范围内借用的正确写法。

## 练习

1. 把 main.rs 的 owned 改成直接保存 &str，再把使用位置放到 replace_text 后，读出编译错误中的两处借用位置。
2. 在短期 fallback 的块内把 choose_preview 的结果转为 String，带到块外使用。
3. 解释为何 read_document(reader) 与 read_document(&mut reader) 的 Drop 计数不同。

本课目标：看到一个返回值，能说明它拥有还是借用数据；看到一个资源，能指出谁拥有它以及释放发生的位置。
