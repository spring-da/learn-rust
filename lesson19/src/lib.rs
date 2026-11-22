//! 第十九课：静态分发、动态分发与 dyn compatibility。
//!
//! 返回位置的 impl Sender 仍要求同一个函数返回一种具体类型：
//!
//! ```compile_fail,E0308
//! use lesson19::{EmailSender, Sender, SmsSender};
//! fn choose(email: bool) -> impl Sender {
//!     if email { EmailSender::default() } else { SmsSender::default() }
//! }
//! ```
//!
//! 含未排除出动态调用面的泛型方法的 trait 不能变成 dyn：
//!
//! ```compile_fail,E0038
//! trait Encoder {
//!     fn encode<T: std::fmt::Display>(&self, value: T) -> String;
//! }
//! fn inspect(_: &dyn Encoder) {}
//! ```
//!
//! 默认的 T: Sender 包含 Sized，不能直接让 T=dyn Sender：
//!
//! ```compile_fail,E0277
//! use lesson19::{EmailSender, Message, Sender, send_batch};
//! let mut sender: Box<dyn Sender> = Box::new(EmailSender::default());
//! send_batch(&mut *sender, &[Message::new("dev", "hello")]).unwrap();
//! ```
//!
//! fresh 被 Self: Sized 排除出动态调用面，不能通过 dyn Sender 调用：
//!
//! ```compile_fail
//! use lesson19::{EmailSender, Sender};
//! let sender: Box<dyn Sender> = Box::new(EmailSender::default());
//! let fresh = sender.fresh();
//! ```
//!
//! ?Sized 放宽隐含约束，允许 T=dyn Sender；内部调用此时仍使用动态分发：
//!
//! ```
//! use lesson19::{EmailSender, Message, Sender, send_batch_flexible};
//! let mut sender: Box<dyn Sender> = Box::new(EmailSender::default());
//! let receipts = send_batch_flexible(&mut *sender, &[Message::new("dev", "hello")]).unwrap();
//! assert_eq!(receipts[0].channel, "email");
//! ```
//!
//! 具体类型上可调用 fresh；整个 Sender trait 仍然可以用于 dyn：
//!
//! ```
//! use lesson19::{EmailSender, Message, Sender};
//! let email = EmailSender::default();
//! let fresh = email.fresh();
//! let mut sender: Box<dyn Sender> = Box::new(fresh);
//! assert_eq!(sender.send(&Message::new("dev", "hello")).unwrap().sequence, 1);
//! ```

use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub recipient: String,
    pub text: String,
}

impl Message {
    pub fn new(recipient: impl Into<String>, text: impl Into<String>) -> Self {
        Self { recipient: recipient.into(), text: text.into() }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Receipt {
    pub channel: &'static str,
    pub recipient: String,
    pub text: String,
    pub sequence: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    InvalidMessage(&'static str),
    UnknownChannel(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMessage(reason) => write!(formatter, "invalid message: {reason}"),
            Self::UnknownChannel(kind) => write!(formatter, "unknown channel: {kind:?}"),
        }
    }
}

impl Error for SendError {}

pub trait ChannelNamer {
    fn channel(&self) -> &'static str;
}

pub trait Sender: ChannelNamer {
    fn send(&mut self, message: &Message) -> Result<Receipt, SendError>;

    // 这个方法只供 Sized 的具体实现使用，不参与 trait object 的动态调用。
    fn fresh(&self) -> Self where Self: Sized;
}

#[derive(Default)]
pub struct EmailSender { sent: usize }

#[derive(Default)]
pub struct SmsSender { sent: usize }

impl EmailSender {
    pub fn sent(&self) -> usize { self.sent }
}

impl SmsSender {
    pub fn sent(&self) -> usize { self.sent }
}

impl ChannelNamer for EmailSender {
    fn channel(&self) -> &'static str { "email" }
}

impl ChannelNamer for SmsSender {
    fn channel(&self) -> &'static str { "sms" }
}

// 显式 impl；仅有同名的固有方法不会自动实现 trait。
impl Sender for EmailSender {
    fn send(&mut self, message: &Message) -> Result<Receipt, SendError> {
        validate_message(message)?;
        self.sent += 1;
        Ok(make_receipt(self.channel(), self.sent, message))
    }

    fn fresh(&self) -> Self { Self::default() }
}

impl Sender for SmsSender {
    fn send(&mut self, message: &Message) -> Result<Receipt, SendError> {
        validate_message(message)?;
        self.sent += 1;
        Ok(make_receipt(self.channel(), self.sent, message))
    }

    fn fresh(&self) -> Self { Self::default() }
}

fn validate_message(message: &Message) -> Result<(), SendError> {
    if message.recipient.trim().is_empty() {
        return Err(SendError::InvalidMessage("recipient is empty"));
    }
    if message.text.trim().is_empty() {
        return Err(SendError::InvalidMessage("text is empty"));
    }
    Ok(())
}

fn make_receipt(channel: &'static str, sequence: usize, message: &Message) -> Receipt {
    Receipt {
        channel,
        recipient: message.recipient.clone(),
        text: message.text.clone(),
        sequence,
    }
}

// T 默认要求 Sized；用具体类型调用时采用静态分发。
pub fn send_batch<T: Sender>(sender: &mut T, messages: &[Message]) -> Result<Vec<Receipt>, SendError> {
    send_batch_flexible(sender, messages)
}

// 放宽 Sized 后，也可以让 T=dyn Sender；这时 send 调用采用动态分发。
// 遇到错误停止，已生成的回执计数不回滚。
pub fn send_batch_flexible<T: Sender + ?Sized>(
    sender: &mut T,
    messages: &[Message],
) -> Result<Vec<Receipt>, SendError> {
    let mut receipts = Vec::with_capacity(messages.len());
    for message in messages {
        receipts.push(sender.send(message)?);
    }
    Ok(receipts)
}

// 不同具体类型统一保存为 Box<dyn Sender>；逐项记录结果，一项错误不终止其他项。
pub fn dispatch(
    senders: &mut [Box<dyn Sender>],
    message: &Message,
) -> Vec<Result<Receipt, SendError>> {
    let mut results = Vec::with_capacity(senders.len());
    for sender in senders {
        results.push(sender.send(message));
    }
    results
}

pub fn make_sender(kind: &str) -> Result<Box<dyn Sender>, SendError> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "email" => Ok(Box::new(EmailSender::default())),
        "sms" => Ok(Box::new(SmsSender::default())),
        _ => Err(SendError::UnknownChannel(kind.into())),
    }
}

pub fn optional_sender(enabled: bool) -> Option<Box<dyn Sender>> {
    if enabled { Some(Box::new(EmailSender::default())) } else { None }
}

pub struct ChannelLabel(pub &'static str);

impl ChannelNamer for ChannelLabel {
    fn channel(&self) -> &'static str { self.0 }
}

pub fn describe(channel: &dyn ChannelNamer) -> String {
    format!("channel={}", channel.channel())
}

#[cfg(test)]
mod tests;
