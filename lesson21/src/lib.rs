//! 私有模块可以通过 pub use 提供稳定的公共入口。
//!
//! ```
//! use lesson21::{MemorySender, send_text};
//! let mut sender = MemorySender::new("local").unwrap();
//! assert_eq!(send_text(&mut sender, "  hello  ").unwrap().text, "hello");
//! ```
//!
//! ```compile_fail,E0603
//! use lesson21::model::Message;
//! ```
//!
//! ```compile_fail,E0616
//! let message = lesson21::Message::new("hello").unwrap();
//! println!("{}", message.text);
//! ```
//!
//! ```compile_fail,E0603
//! lesson21::normalize_channel("local").unwrap();
//! ```

mod adapters;
mod model;
mod service;

pub use adapters::MemorySender;
pub use model::{Error, Message, Receipt};
pub use service::{Sender, send_text};

pub(crate) fn normalize_channel(channel: &str) -> Result<String, Error> {
    let channel = channel.trim();
    if channel.is_empty() { return Err(Error::InvalidChannel); }
    Ok(channel.into())
}
