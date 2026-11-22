# 第 21 课：package、crate、module 与可见性

本课把通知案例拆成模块，通过稳定的公开入口调用私有实现。

```powershell
Set-Location E:\WorkProject\rust\lesson21
cargo run --offline
cargo test --offline
```

一次运行本组第 21～23 课：`& 'E:\WorkProject\rust\lesson21\run-batch.ps1'`。
整组路线见 [LEARNING_PATH.md](LEARNING_PATH.md)。

```text
lesson21/
  Cargo.toml
  src/lib.rs                库 crate 根、mod 声明、pub use
  src/main.rs               二进制 crate 根、组装入口
  src/model.rs              Message、Receipt、Error
  src/service.rs            Sender 与 send_text
  src/adapters.rs           MemorySender
  tests/public_api.rs       作为外部 crate 测试公开 API
```

| 概念 | 含义 |
| --- | --- |
| Cargo package | Cargo.toml 管理的包，可包含库和二进制等目标 |
| crate | 一次编译的单元，本课 lib.rs 与 main.rs 属于不同 crate |
| module | crate 内部的命名与可见性层次 |
| mod model; | 将模块纳入编译，按规则寻找其源文件 |
| use | 引入路径，便于当前作用域使用 |
| pub use | 重新导出，形成面向调用方的入口 |

把文件放进 src 不等于它已经被声明为模块。
src/main.rs 中的 `crate::` 指二进制自身；`lesson21::` 指依赖的库 crate。
[Rust 模块说明](https://doc.rust-lang.org/book/ch07-02-defining-modules-to-control-scope-and-privacy.html)

## 私有模块仍可提供公开入口

lib.rs 声明 `mod model;`，但通过 `pub use model::{Message, Receipt, Error};` 导出选定的类型。
调用方使用 `lesson21::Message`，不需要知道内部文件与模块路径。
将来改变内部文件组织，可以继续保留这个公共入口。

| 可见性 | 本课如何理解 |
| --- | --- |
| 默认私有 | 当前模块及后代模块可访问，仍受具体成员规则约束 |
| pub | 通过可访问路径向外公开 |
| pub(crate) | 当前 crate 内可用 |
| pub(super) | 限制在父模块范围 |

normalize_channel 使用 pub(crate)，适配器可以调用，外部 crate 不可以调用。
即使 main.rs 与 lib.rs 位于同一 Cargo package，二进制也不能直接访问库的 pub(crate) 项。
[Rust 可见性规则](https://doc.rust-lang.org/reference/visibility-and-privacy.html)

## 构造与不变量

Message 的 text 字段私有，只通过 new 构造。去首尾空白后要求 1～120 个 UTF-8 字节。
本类型没有实现 Default，外部也不能通过结构体字面量写入私有字段。
相比配套 Go 中仍能创建 Message{} 零值的情况，这个构造边界更强。

send_text 依赖 Sender trait，MemorySender 实现它；新增实现可以保持业务函数不变。
Receipt 的公开字段作为结果数据，允许调用方直接读取。

## 测试就是一次真实的外部调用

tests/public_api.rs 作为独立测试 crate，只能使用公开入口。
三个集成测试验证构造边界、状态和外部 Sender 实现。
四个文档测试中，一个验证公开调用，三个确认编译器拒绝私有模块、私有字段与 pub(crate) 函数的访问。

## 动手练习

1. 把一个未使用的文件加入 src，观察未写 mod 声明时是否参与编译。
2. 新增适配器并重新导出，保持调用方仍从 lesson21 根路径导入。
3. 将 normalize_channel 改成私有，预测哪个模块会先产生编译错误。

验收：区分 Cargo package、crate 和 module，并能解释 pub use 与 pub(crate) 的作用。
