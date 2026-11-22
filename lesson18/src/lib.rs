//! 所有权、借用、生命周期与资源释放。
//!
//! 不能返回对局部 String 内容的引用；标注 'a 也不能让它延长寿命：
//!
//! ```compile_fail,E0515
//! fn invalid<'a>() -> &'a str {
//!     let text = String::from("temporary document");
//!     &text
//! }
//! ```
//!
//! 借用的行不能在 Document 销毁之后继续使用：
//!
//! ```compile_fail,E0597
//! use lesson18::Document;
//! let line;
//! {
//!     let doc = Document::new(String::from("TITLE: temporary"));
//!     line = doc.first_line();
//! }
//! println!("{line}");
//! ```
//!
//! 仍要使用旧借用时，不能替换拥有者的文本：
//!
//! ```compile_fail,E0502
//! use lesson18::Document;
//! let mut doc = Document::new(String::from("TITLE: old"));
//! let line = doc.first_line();
//! doc.replace_text(String::from("TITLE: new"));
//! println!("{line}");
//! ```
//!
//! 返回值可能来自任一输入，因此不能超过短期 fallback 的有效借用范围：
//!
//! ```compile_fail,E0597
//! use lesson18::choose_preview;
//! let text = String::from("TITLE: nonempty");
//! let preview;
//! {
//!     let fallback = String::from("fallback");
//!     preview = choose_preview(&text, &fallback);
//! }
//! println!("{preview}");
//! ```
//!
//! 复制为拥有数据的 String，可以独立于 Document 保存：
//!
//! ```
//! use lesson18::Document;
//! let doc = Document::new(String::from("TITLE: retained\nbody"));
//! let owned_line = doc.first_line().to_owned();
//! drop(doc);
//! assert_eq!(owned_line, "TITLE: retained");
//! ```
//!
//! 两个输入可以有不同的实际存活范围；只要在共同有效范围内使用输出即可：
//!
//! ```
//! use lesson18::choose_preview;
//! let text = String::from("TITLE: long lived");
//! {
//!     let fallback = String::from("short lived");
//!     assert_eq!(choose_preview(&text, &fallback), "TITLE: long lived");
//! }
//! ```

pub mod cleanup;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug)]
pub struct Document {
    text: String,
}

impl Document {
    pub fn new(text: String) -> Self {
        let doc = Self { text };
        doc // 将拥有数据的 Document 移动给调用方。
    }

    // 生命周期省略规则将返回引用关联到 &self。
    pub fn first_line(&self) -> &str {
        first_line(&self.text)
    }

    pub fn replace_text(&mut self, text: String) {
        self.text = text;
    }
}

pub fn first_line(text: &str) -> &str {
    let line = text.split_once('\n').map_or(text, |(head, _)| head);
    line.strip_suffix('\r').unwrap_or(line)
}

pub fn choose_preview<'a>(text: &'a str, fallback: &'a str) -> &'a str {
    let line = first_line(text);
    if line.is_empty() { fallback } else { line }
}

pub fn load_document(path: impl AsRef<Path>) -> io::Result<Document> {
    read_document(File::open(path)?)
}

// 按值接收 reader，接管它的所有权；正常返回和 ? 提前返回都会释放拥有的 reader。
pub fn read_document(mut reader: impl Read) -> io::Result<Document> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    let doc = Document::new(text);
    if !doc.first_line().starts_with("TITLE:") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "first line must start with TITLE:",
        ));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests;
